# WebAssembly backend for Sonatina and Fe: context

This document is for whoever takes the WebAssembly work over, people or agents. The briefing is the first section and can be read alone. The appendices hold the detail: where everything is, how to build and check it, how it works, what was measured, what is missing, and what comes next.

Conventions. "Observed" means a command was run on the named revision and the result is stated. "Read" means taken from source or text without running it. "Not tested" means exactly that. Numbers from the earlier, older base are labelled as such. Square brackets with double brackets, such as `[[this]]`, mark a value that is filled in when a run finishes; none should remain in the published version.

## Briefing

### What this is

A WebAssembly (wasm32) backend for Sonatina, and the Fe side that uses it. Fe can compile a standalone file to a WebAssembly module (`fe build --backend wasm`), run its tests as WebAssembly modules under Node (`fe test --backend wasm`), and write a manifest that describes each module. WebAssembly is a third compilation target of the Fe compiler, next to the EVM and native code, with a 32-bit `usize`.

The work was done on an earlier base, then re-applied onto the current Sonatina main and Fe master. The two branches below are that re-application. Older branches are kept as records only.

### Where it is

| Repository | Branch | Built on | Commits |
| --- | --- | --- | --- |
| Sonatina | `feat/wasm-crate-20261030` (https://github.com/micahscopes/sonatina/tree/feat/wasm-crate-20261030) | main at `c30e6b22` (29 October) | 5 |
| Fe | `feat/wasm-backend-on-main-20261030` (https://github.com/micahscopes/fe/tree/feat/wasm-backend-on-main-20261030) | master at `e572b31ac7` (29 October) | 10 |

The Sonatina commits: the wasm32 triple and ISA with native layout parameterized by pointer width; a fix in the aggregate scalarizer (it affects every backend); a new crate, `sonatina-wasm`, next to `sonatina-cranelift`, which uses only the public API of `sonatina-codegen`; a heap that reuses freed memory, with size classes and free lists, replacing an arena that never freed; and an option that fills freed blocks with a pattern, to find use after free.

The Fe commits: the compilation target and its lowering; the command-line interface; `std::wasm` with the host effects, the files contract and a Node host; the tests; the workflow and check script; a follow-up that keeps three storage-layout tests within a 32-bit `usize` and another for five layout size tests; the change that makes `free` and fallible allocation use the allocator of the module, with a test that a loop of allocate and drop stays bounded; and a pin of the Sonatina crates to the revision that contains `sonatina-wasm`.

Appendix A lists every branch, including the older records and the work-in-progress branches for other streams.

### Try it

Requirements: Node.js 20 or later (22 was used), and `wasm-tools` for the check script.

```sh
# Sonatina: the backend crate, its tests, and the comparison with the native JIT
cargo test -p sonatina-wasm

# Fe: build the compiler with the backend, then run every check
cargo build --release -p fe --features wasm,cranelift
bash examples/wasm/check.sh target/release/fe

# Build one program
target/release/fe build --backend wasm --out-dir out examples/wasm/lines.fe
node examples/wasm/lines.mjs out/lines.wasm examples/wasm/README.md
```

Appendix B has the full commands, the test files, and the traps that cost time.

### State

Observed on the rebased pair before it was pushed:

- Sonatina: 105 backend tests, the differential test against the native JIT, and the documentation test pass. The new scalarizer test fails when its one-line fix is removed and passes with it. Formatting and lints (as errors) are clean.
- Fe: the seven WebAssembly test files pass (149 tests). `examples/wasm/check.sh` passes (19 Node tests; 128 modules built, validated and checked against their manifests; 20 reproducible builds; 4 of 4 tests on WebAssembly and 4 of 4 on native). Formatting and lints (as errors, whole workspace, all features) are clean. The codegen and corelib tests pass.
- Full Fe workspace run: `cargo test --workspace --all-features --no-fail-fast` was run once, before the heap was added, and took 57 minutes. It reported five failing targets. Three failed because their input was an array length above `2**32 - 1`, which is no longer a valid `usize` literal once `usize` has 32 bits on one target: the command-line "type too large" test, `layout_extent_overflow_rejects_without_panicking` in the `fe-hir` layout tests, and two language-server layout hover tests. A fourth test of the same kind, `oversized_type_size_is_rejected` in the `fe-hir` frontend target, could not run because that target aborted. All four now use a pair of 32-bit lengths with the same total (for example `[[u256; 0x80000000]; 0x10000000]` for 2**59 elements) and pass: the command-line test, 11 layout tests, 113 language-server tests, and the `fe-hir` frontend target with 170 tests. The other two failures were debug-build stack overflows, `deeply_nested_malformed_type_paths_are_resolved_once` in the `fe-hir` frontend target and shard 1 of `semantic_access`. Both abort in the same way on plain master `e572b31ac7` with the default stack, so they are not caused by this work, and both pass with `RUST_MIN_STACK=67108864`. The whole workspace was not rerun after the heap was added; after it, the seven WebAssembly test files, the example script and the Sonatina crate tests were run (F.1, F.2)
- Browser check on the rebased pair: passed. The 512 by 512 Mandelbrot kernel (fixed-point, `-O s`, one exported function and the memory, no imports) was built with the rebased compiler. In Node, all 262,144 pixels match an independent BigInt reference (0 mismatches; 45,152 pixels reach the iteration limit). The same module then ran in headless Chrome 141 (desktop, no display): the 1,048,576 bytes read back from the canvas equal the Node frame byte for byte (0 differing). The frame hash (FNV-1a-32 over the pixels) is `0x2d29649a`, the same as on the earlier base. The check was repeated with the final compiler, after the heap was added, and gave the same result; the module bytes are identical to the earlier build, as expected for a program that allocates nothing. This is the only browser check; `host.mjs` has not been run in a browser, and no phone browser has been tried.

Appendix F has the counts per file and what was not run.

### What is left

In rough order. Each item points to the appendix with the detail.

1. The memory model is done in the way described in Appendix O, and what remains is smaller. Freed blocks are reused: a loop that allocates and drops a 400-byte block used to grow the memory by 381 MiB per million rounds and now does not grow it; `free` and fallible allocation use the allocator of the module; a fallible allocation that cannot be met returns `None`. Still open: the memory is never returned to the engine, there is no `realloc`, a block takes up to twice its request, free blocks are not merged (so a program that allocates many small blocks and then many large ones keeps both), and a few optional items from the plan were not built (Appendix O.7). Appendices C, G.1, I.3, K.3, O.

2. Land it. Decide how the commits become pull requests (Sonatina first, then Fe pinned to the merged revision). Appendices A, E, K.
3. The `usize` rule. Limiting `usize` literals to 32 bits on every target, EVM included, is the simplest rule that keeps casts lossless. Appendix E.
4. The optimization pipeline for wasm32, native or size-oriented. Re-measured on the rebased pair: module sizes at `-O 1` and `-O 2` are far smaller than on the earlier base because the recursion growth is gone, `-O s` is still smallest or close to it in most programs, and programs with checked arithmetic grew 7 to 31 percent (G.2). Appendix G.
5. Rich values across the host boundary and Web IDL. This layer has scalars, text in, a tagged reply and a files contract. A separate, larger body of work on the MB2 branches covers rich values, resources, callbacks and a Web IDL generator. Appendix J has the links and an ordered plan.
6. Floating point. Undecided: a primitive type, or a library over bits. Appendix K.
7. Open checks: the full workspace was not rerun after the heap was added (only the WebAssembly test files, the example script and the Sonatina crate); the whole `sonatina-codegen` suite and the EVM suites were not run on the rebased Sonatina branch; the frame cost after an import error was not rechecked; run times were not re-measured Appendix I.

### Decisions already taken

Third compilation target with a 32-bit `usize`. One line of development; earlier branches are records. The layer is self-contained: nothing in it depends on the code-generation, suspension or `Backend` layers (checked with git against the draft pull request heads and the integration branches). The layer is re-applied, not merged. Appendix L has the dates.

### Appendix index

A Branches and records. B Build, run and verify. C The Sonatina crate. D The Fe side. E Changes to shared code and the rebase. F Checks and results. G Measurements. H Comparison with the WebAssembly prototype on the maintainer's branch. I Known gaps and limits. J Web IDL and the MB2 integration. K Ordered backlog. L Decisions. M Glossary. N Revisions and provenance. O Plan for a freeing allocator.

---

## Appendix A: Branches and records

### A.1 The rebased pair

Sonatina `feat/wasm-crate-20261030` (https://github.com/micahscopes/sonatina/tree/feat/wasm-crate-20261030), Fe `feat/wasm-backend-on-main-20261030` (https://github.com/micahscopes/fe/tree/feat/wasm-backend-on-main-20261030). Base revisions: Sonatina main `c30e6b22e57ab5315e634539a5e072ee41c67b17`, Fe master `e572b31ac72231b5d740ffed1b551ee63f360e8b`.

The Sonatina branch has five commits:

1. Add the wasm32 target triple and ISA and parameterize the native type layout by pointer width (`ir`, `triple`, `parser` crates).
2. Skip a phi that is its own user when scalarizing a loop-carried aggregate (`codegen`, one condition and one test).
3. Add the `sonatina-wasm` crate with the WebAssembly backend (about 11,800 lines, of which about 5,000 are tests).
4. Reuse freed memory in the WebAssembly backend with size classes and free lists (`heap.rs`, about 780 lines, and about 700 lines of tests).
5. Add an option that fills freed blocks with a pattern (`with_heap_poison`).

The Fe branch has ten commits:

1. Add a WebAssembly compilation target with a 32-bit usize (`common`, `mir`, `codegen`, core library, snapshots, news fragment).
2. Add `fe build --backend wasm` and `fe test --backend wasm` (`crates/fe/src`, `CLI.md`).
3. Add `std::wasm` with host effects, the files contract and a Node host (`ingots/std`, `examples/wasm`).
4. Test programs built with the WebAssembly backend (`crates/fe/tests`).
5. Add a WebAssembly workflow and a script that runs the checks (`.github`, `examples/wasm`).
6. Keep the storage layout tests within a 32-bit usize (`crates/uitest`).
7. Use usize literals that fit 32 bits in the layout size tests (`crates/fe`, `crates/hir`, `crates/language-server`).
8. Free memory on WebAssembly through the allocator of the module (`lower_runtime.rs`, `CLI.md`, `examples/wasm/README.md`, `ptr.fe`).
9. Test that freed memory is reused on WebAssembly (the `heap_churn` fixture and tests).
10. Pin Sonatina to the revision that contains the sonatina-wasm crate (`Cargo.toml`, `Cargo.lock`).

The commits are grouped by purpose. Only the final state of each branch was built and tested; an intermediate commit was not built on its own.

The Fe branch compiles against a Sonatina revision that contains the `sonatina-wasm` crate. See B.3 for how that is satisfied.

### A.2 The older line and its records

Before the rebase the work lived on a "merged line": the author's WebAssembly backend merged with the maintainer's native pointer-address work and the Sonatina native-performance branch. It is `wip/wasm-on-native-perf-20261027` in both repositories. The Fe version contains 73 commits beyond master: 37 are the maintainer's (the pointer-address work) and 36 are the author's. The Sonatina version contains 63 beyond main: 35 are the maintainer's and 28 the author's. These branches carry the author's commit-by-commit history; the rebased branches carry a re-application of the same content in fewer commits.

Five commits at the bottom of the older Sonatina line are not about WebAssembly. They are native lifetime-rejection tests and a small change in the Cranelift backend and the aggregate pass (`18d70e62`, `5f2c28b4`, `6a7c8029`, `111eaa76`, `1739931e`), inherited from the branch `wip/native-product-on-main-20261026`. They are not in the rebased Sonatina branch (observed: `1739931e` is not an ancestor).

The staged branches show how the work grew:

| Stage | Content |
| --- | --- |
| S1 | Integer programs in Sonatina; tests against the IR semantics |
| S2 | Scalar Fe programs with `fe build --backend wasm` |
| S3 | Aggregates, constants and dynamic memory |
| S4 | Imports and `std::wasm` host effects |
| S5 | The files contract, text parameters, the reply result and the Node host |
| S6 | The build manifest, reproducibility checks, `fe test --backend wasm`, the workflow and the README |

Two branches published earlier, before this stream, are `feat/wasm-backend-20261026` on both forks (Fe at `1b7ca2db7f`, Sonatina at `96d2c05d`). They hold stages S1 to S5 on older bases and were not changed.

### A.3 Index of the pushed records

All of these are on the author's forks (`github.com/micahscopes/fe` and `github.com/micahscopes/sonatina`) under the `wip/` prefix. "Commits beyond master" counts from the current Fe master, so branches built on the maintainer's pointer-address work include those commits. Newest-commit subjects are taken from git.

@@BRANCH_TABLES@@

The layer branches, the generation work and the integration branches in the second table belong to other streams. They are listed so that nothing is lost; the WebAssembly layer does not depend on them (observed: none of their head commits is in the history of the rebased Fe branch). The integration branches were described as throwaway sketches.

---

## Appendix B: Build, run and verify

### B.1 Tools

| Tool | Version used | Needed for |
| --- | --- | --- |
| Rust | cargo 1.100.0 nightly 2026-08-17 (Sonatina requires 1.97 or later) | everything |
| Node.js | 22 (20 or later is required) | running modules, `host.mjs`, the check script, `fe test --backend wasm` |
| `wasm-tools` | 1.247.0 | independent validation in `check.sh` |
| Headless Chrome | 141 | the one browser check |

### B.2 Sonatina

```sh
cargo check -p sonatina-wasm --all-targets
cargo test  -p sonatina-wasm          # 105 backend tests, the differential test, the documentation test
cargo test  -p sonatina-ir -p sonatina-triple -p sonatina-parser
cargo test  -p sonatina-codegen --lib scalarize
cargo fmt --all -- --check
cargo clippy -p sonatina-wasm --all-targets -- -D warnings
```

The differential test (`wasm_differential`) generates programs over structs, arrays and objects, runs them on the host-native Cranelift JIT and on the WebAssembly backend, and compares the results. It takes about two minutes. It depends on `sonatina-cranelift` with its `jit` feature. The shared oracle data in `crates/codegen/tests/alias_execution/` is included by path from the wasm tests, not copied.

Trap: two Sonatina worktrees that share one `CARGO_TARGET_DIR` can reuse each other's compiled `triple` crate and fail to parse a triple. Use one target directory per worktree.

### B.3 Fe

The Fe workspace pins all the Sonatina crates, including `sonatina-wasm`, to one revision on the author's fork of Sonatina (`https://github.com/micahscopes/sonatina`, the revision that contains the crate), so a plain build needs nothing else. When the crate is merged, the pin moves back to the Sonatina repository. To develop Fe and Sonatina together, a local, uncommitted patch in `.cargo/config.toml` in the Fe checkout replaces the pinned crates with a checkout:

```toml
[patch."https://github.com/micahscopes/sonatina"]
sonatina-ir        = { path = "<sonatina>/crates/ir" }
sonatina-triple    = { path = "<sonatina>/crates/triple" }
sonatina-codegen   = { path = "<sonatina>/crates/codegen" }
sonatina-verifier  = { path = "<sonatina>/crates/verifier" }
sonatina-sp1       = { path = "<sonatina>/crates/sp1" }
sonatina-macros    = { path = "<sonatina>/crates/macros" }
sonatina-parser    = { path = "<sonatina>/crates/parser" }
sonatina-cranelift = { path = "<sonatina>/crates/cranelift" }
sonatina-wasm      = { path = "<sonatina>/crates/wasm" }
```


The `wasm` feature of `fe` and `fe-codegen` enables `sonatina-wasm`. The `cranelift` feature is needed only to compare `fe test` results with the host-native backend.

```sh
cargo build --release -p fe --features wasm,cranelift
bash examples/wasm/check.sh target/release/fe

# The seven WebAssembly test files
cargo test -p fe --features wasm,cranelift \
  --test wasm_backend --test wasm_files --test wasm_imports --test wasm_manifest \
  --test wasm_memory --test wasm_reproducible --test wasm_test_command

cargo test -p fe-uitest --all-features --no-fail-fast
cargo test -p fe-codegen --all-features
cargo test -p fe-driver --test corelib
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Traps:

- The `wasm_*` test files are compiled out without the `wasm` feature and then report zero tests, which looks like a pass. Always pass the feature.
- `cargo test` stops at the first failing test target. Use `--no-fail-fast` to see every target.
- A test target that overflows the stack in a debug build prints no `test result` line. A missing result line is a failure to investigate, not a pass.
- `fe test --backend wasm` starts one Node process per test and ends a test after 60 seconds.
- Output of the form `panicked: arithmetic overflow` in the WebAssembly test logs comes from tests that trigger traps on purpose.

### B.4 Snapshot tests

Uitest and Sonatina IR snapshot tests use `insta`. A changed output is written next to the snapshot as `.snap.new`. Read the difference before accepting it, then move the new file over the old one. Keep the existing `assertion_line:` header line in its place so the diff holds only real changes.

---

## Appendix C: The Sonatina crate

The crate is `crates/wasm`, package `sonatina-wasm`. It depends on `sonatina-codegen`, `sonatina-ir`, `sonatina-triple`, `sonatina-verifier`, `rustc-hash` and `waffle` 0.3.3, which recovers structured control flow and encodes the module. Its tests add `wasmtime` and `wasmparser`.

From `sonatina-codegen` it uses four public items: `Backend` and `BackendOptions`, `Pipeline`, `EnumLowerToProduct`, and `check_frame_lifetimes`. Nothing in `codegen` had to change for it except the scalarizer fix.

### C.1 Files

| File | Role | Lines |
| --- | --- | --- |
| `src/lib.rs` | `WasmBackend`, its options, errors, the pipeline | about 190 |
| `src/translate.rs` | instruction translation, calls, exports and imports | about 2,400 |
| `src/translate/aggregate.rs` | structs, arrays, objects, constants, `gep` | about 980 |
| `src/translate/wide_ops.rs` | 256-bit operations that need more than one step | about 310 |
| `src/arith.rs` | integer arithmetic at each width, overflow checks | about 580 |
| `src/memory.rs` | the layout of memory, global data and the runtime globals | about 260 |
| `src/heap.rs` | the allocator: `malloc`, `free`, the host `alloc`, `arena_release` | about 780 |
| `src/wide.rs` | 256-bit values as four 64-bit limbs | about 880 |
| `tests/wasm_backend.rs` and `tests/wasm/*.rs` | engine-executed tests | about 5,000 |
| `tests/wasm_differential.rs` | generated programs against the native JIT | about 550 |
| `README.md` | the specification of the target | about 240 |

### C.2 Specification

The README of the crate is the specification and should be read in full. The main points:

- Types: `i1`, `i8`, `i16`, `i32` are WebAssembly `i32`; `i64` is `i64`; pointers and references are `i32` offsets into linear memory; structs and arrays are the `i32` address of the value in memory; `i256` is four `i64` limbs, least significant first. `i128`, enum tags, `unit` and function types cannot be parameters, results, phi values or operands.
- Narrow integers are held zero-extended in their `i32`. Parameters of exports and results of imports are canonicalized on entry, so a host may pass any bits above the width.
- Memory layout, lowest address first: 16 reserved bytes; global variables (private only); the table of free-list heads (128 bytes, when the module has a heap); the stack (1 MiB by default, set with `with_stack_size`); the heap up to the end of memory.
- Frames: every function takes the address of the first free stack byte as a hidden first parameter and places static slots there. There is no mutable stack pointer, so a trap leaves nothing to repair. A call whose frame does not fit traps. A call to an import stores the end of the caller's frame in the `stack_top` global so that an export the host calls back runs above it, and restores it on return. When the host throws during an import, the caller's frame stays reserved: each such error costs the frame of one function (documented in the README; not rechecked on this base).
- The heap: size classes and free lists (O has the design). Every block has a 16-byte header (class and state) and a payload of 2**k bytes for k from 4 to 31. A freed block goes on the list of its class and is the first reused; blocks are cut from the top of the heap and are never split or merged. The heap grows with `memory.grow` up to the 4 GiB of a 32-bit address space and never gives memory back. The external C routines `malloc` and `free` are the allocator of the module and not imports: `malloc` returns null when the request is above 2 GiB, would not fit in 32 bits, or cannot be met because the memory will not grow, and `mem.alloc_dynamic` traps then; `free` traps on an address that is not a block in use, which catches a double free. A block taken from a list is not zeroed; a block cut from the top is.
- Host allocator (`with_host_allocator`): exports `alloc(size)` (a zeroed block cut from the top, never taken from a list), `arena_mark()` (the top of the heap) and `arena_release(mark)` (frees every block at or above the mark, unlinking free blocks that end above it from their lists). A host places text with `alloc`, makes the call, reads results, and releases the mark.
- Aggregates: a struct or array value is the address of an immutable snapshot. Functions take aggregates by address and return one aggregate through a buffer in the caller's frame.
- 256-bit arithmetic: `add`, `sub`, `mul`, `neg`, `udiv`, `sdiv`, `umod`, `smod`, shifts, bitwise operations, all comparisons, extensions and truncations, the checked forms (`uaddo`, `usubo`, `umulo`, `saddo`, `ssubo`, `smulo`, `snego`), `bswap` and pointer conversions. Multiplication, division and variable shifts are module functions emitted only when used. Division is one 64-bit division when both operands fit, and otherwise one step per bit of the dividend. The saturating forms are not translated.
- Semantics follow the IR interpreter and the native backend: wrapping arithmetic, shifts by at least the width give zero or the sign fill, division by zero traps, `br_table` traps when no case matches, `undef` is zero.
- Order of effects: reads, writes and calls happen in program order. A test generates straight-line programs mixing them and compares results and final memory with a model at every optimization level.
- Unsupported input is rejected with an error that names it, and the backend does not panic: `get_function_ptr`, `sym_addr`, `sym_size`, `i128`, function types and enum tags as values, public and external global variables.

### C.3 Instructions added to Sonatina main that this crate had to learn

When the layer was re-applied, three instructions were missing from the translation. Each has tests.

- `memcopy(dest, src, len)`: `memory.copy`, which allows overlapping ranges. Test: overlapping copies in both directions, a zero length, and an out-of-range trap.
- `freeze(arg)`: the identity, because an undefined value is already zero in WebAssembly and every use sees one value. Tests at every integer width and for 256-bit values.
- `evm_clz(word)`: leading zeros at the width of the operand. Narrow widths use `i32.clz` reduced by the padding; `i1` is the exclusive-or with 1; 256-bit values are counted limb by limb with selects. Tests at every width and for 256-bit values against a reference.

### C.4 Implementation choices worth knowing

- The overflow check of a 64-bit multiply uses three multiplications on 32-bit halves and no division. The earlier division-based check (`(a*b)/a != b`) cost about twice as much on a modular-exponentiation kernel (57 ms down to 25 ms in Node on the earlier base). A planted bug in the carry term makes the tests fail.
- `addmod` and `mulmod` on 256-bit values are bit-serial: exact, slow.
- Pointer-to-integer and integer-to-pointer conversions on 256-bit values trap when the value does not fit in 32 bits.
- The same module compiled twice gives identical bytes.

---

## Appendix D: The Fe side

### D.1 Pipeline

`fe build --backend wasm file.fe` reads one standalone file. Every non-generic `pub fn` in the root becomes an export. The compiler lowers the program to the runtime package of exported functions (`mir::build_wasm_export_package`, `crates/mir/src/runtime/package.rs`), lowers that to a Sonatina module with the `Wasm` target (`crates/codegen/src/sonatina/lower_runtime.rs`), compiles it with `sonatina_wasm::WasmBackend` (`crates/codegen/src/sonatina/wasm.rs`), and writes the module and a manifest (`crates/fe/src/wasm_manifest.rs`). `fe test --backend wasm` builds each test into its own module and runs it in Node (`crates/fe/src/test/wasm.rs` and `wasm_runner.mjs`).

### D.2 The target

The `Target` enum has three members, `Evm`, `Native` and `Wasm`, and `usize_bits()` gives 256, 64 and 32. `Target::ALL` drives the rules that must hold on every target: a cast to `usize` must be lossless on all of them (`u64 as usize` is rejected everywhere), and a `usize` literal must fit 32 bits. The allocation limit in the core library follows the target: `u64::max().downcast_saturate()`, which is Solidity's `2**64 - 1` where `usize` is wide enough and the largest `usize` otherwise.

Lowering for `Wasm` differs from the other native targets in these ways (read from `lower_runtime.rs`, tested by the wasm tests):

- No C library is declared. The backend defines the C routines `malloc` and `free` itself, so dropping an owner calls `free` and reuses the memory, and a fallible allocation returns null when the request cannot be met, which the core library turns into `None`. Calls to `memmove` and `memcpy` become `memory.copy` in the backend.
- Panics use `NativePanics::Trap`: a panic ends the module in a trap and the message is not carried.
- A revert on a target without revert data skips the payload.
- Exports are wrapped by adapters (`adapt_exports`, `lower_wasm_adapters`) that map `HostText` parameters and `Reply` results onto the plain WebAssembly signature.
- Calling an `extern` function outside `core` and `std` needs `unsafe`.
- The error for an instruction the target lacks contains "does not support instruction" and, for EVM-only instructions, a note after it. A native fixture test matches that substring.

### D.3 The host boundary

The boundary is deliberately small and versioned (contract version 1).

- Exports take `bool` and integers of up to 64 bits, plus `std::wasm::HostText` parameters (address and length of text the host placed in the module's memory, valid for the call only), and may return a `std::wasm::Reply` (an `i64` with a tag in the low 32 bits and a payload in the high 32; tag 0 is success). Any other type, including `usize`, `u128`, `u256`, structs and pointers, is an error naming the function and parameter.
- Imports: an `extern` function that an export reaches becomes an import of module `env`; one that nothing reaches is not imported. Narrow integers arrive zero-extended. There is no attribute for another module name yet.
- `std::wasm` declares three effects, `Log`, `Clock` and `Abort`, each with a host provider that calls an import and a `mock` provider written in Fe. The imports are `sys_wasm_log_i32`, `sys_wasm_log_u64`, `sys_wasm_now_ms` and `sys_wasm_abort`.
- `std::wasm::files` is a contract with `open(name)`, `size(handle)`, `read_byte(handle, index)` and `close(handle)`, which takes the handle by value so the compiler rejects a second use. Errors are `NotFound` 1, `OutOfRange` 2, `BadName` 3. The compiler does not require a handle to be closed; the host counts open handles.
- A module with a `HostText` parameter exports `alloc`, `arena_mark` and `arena_release`; the three names are reserved.
- `examples/wasm/host.mjs` is a Node host with no dependencies. It supplies the imports, places text, decodes replies, and refuses a forged or closed handle, text outside the memory or longer than 64 KiB, and an unknown error number. Given a manifest it refuses a module whose hash, imports or exports differ, or that imports something the host lacks, naming the cause. It has not been run in a browser.

### D.4 The manifest

`<name>.manifest.json` is written whenever a module is written. It records the module file name, SHA-256 and size, the initial memory in pages, the imports and exports with their Fe and WebAssembly types, the contract version, the optimization level and the compiler version. It contains nothing that depends on the machine, the time or the location of the sources. Two builds of one program with one compiler give identical bytes for the module, its IR and its manifest at every optimization level (`examples/wasm/reproducible.sh` checks 5 files at 4 levels).

### D.5 Command-line behaviour to know

- Built from one `.fe` file; ingots, workspaces, `--ingot` and `--contract` are not supported with this backend.
- Optimization levels `-O 0`, `-O 1` (default), `-O 2`, `-O s`. `--emit wasm,ir` also writes the optimized Sonatina IR as `<name>.wasm.sona`.
- `fe test --backend wasm` supports no `--show-logs`, tracing options, `--debug-dir`, `--emit rmir`, `--report` or `--report-dir`. A test that needs something the backend cannot build (EVM built-ins, `should_revert`, `balance`) fails with `cannot build <name> for WebAssembly` and the reason; nothing is skipped silently.

---

## Appendix E: Changes to shared code, and the rebase

### E.1 What the layer touches outside its own files

Sonatina:

| Area | Change | Why |
| --- | --- | --- |
| `ir` (`isa/native.rs`, `isa/mod.rs`, new `isa/wasm32.rs`) | The native type layout takes the pointer width as a parameter; a wasm32 ISA is added | 4-byte pointers |
| `triple` | `wasm32` architecture | the target triple |
| `parser` | wasm32 target selects 32-bit pointers; one test | modules for the target parse |
| `codegen` `transform/aggregate/scalarize.rs` | A phi that is its own user is skipped when its users are revisited; one test | found by generated programs |
| workspace `Cargo.toml` | `crates/wasm` is a member | the new crate |

The scalarizer fix applies to every backend. A loop-carried struct with a field that is never updated gives that field a leaf phi that names itself on the back edge. The phi is then one of its own users and was looked at again after it had been removed. The test fails without the one-line condition (observed: the planted removal fails the test) and passes with it.

Fe:

| Area | Change |
| --- | --- |
| `crates/common/src/compilation.rs` | `Target::Wasm`, `usize_bits` 32 |
| `crates/mir` (4 files) | runtime package of exported functions; destruction and drops on the new target |
| `crates/codegen` (6 files) | lowering for the target; the backend adapter; its tests |
| `ingots/core/src/ptr.fe` | the allocation limit follows the target |
| `ingots/std/src` | `std::wasm`, `std::wasm::boundary`, `std::wasm::files` |
| `crates/fe/src` | `fe build --backend wasm`, `fe test --backend wasm`, the manifest |
| `crates/uitest/fixtures` | two snapshots moved by lines; three storage-layout tests changed (see E.3) |
| `newsfragments/+portable-usize.removal.md` | text updated for the third target |
| `CLI.md`, `.github/workflows/wasm.yml` | documentation and the workflow |

### E.2 Conflicts when the layer was re-applied onto current master

The layer was applied as one patch with a three-way merge; five files conflicted. A plain merge of the older line conflicted in 63 files (37 without snapshots).

| File | Master's change | The layer's change | Resolution |
| --- | --- | --- | --- |
| `crates/codegen/src/sonatina/lower_runtime.rs`, module construction | construction and native panic definitions return errors | adapter calls added around them | master's error-returning calls, with the adapter calls kept |
| `lower_runtime.rs`, host functions | C library functions declared through a typed `host_function` at the address width | `traps_on_panic`, `is_wasm_target` (a `has_host_libc` that was added at first is gone) | both kept |
| `lower_runtime.rs`, fallible allocation | allocation through `host_function(Malloc)` at the address width | a WebAssembly module does not link the C library | master's body; the module's own `malloc` serves it |
| `lower_runtime.rs`, `memmove` | at the address width | 64-bit length | master's; the backend turns the call into `memory.copy` |
| `ingots/core/src/ptr.fe` imports | `IntWord`, `from_le`, `to_le`, `word_to_usize` | `Bounded`, `IntDowncast` | union of both |
| `crates/codegen/Cargo.toml` | `cranelift` is a separate optional crate | `wasm` feature | `wasm = ["dep:sonatina-wasm"]` |
| two uitest snapshots | none | line numbers in `ptr.fe` | regenerated |

### E.3 Consequence visible to Fe users

With a 32-bit `usize` as a compilation target, a `usize` literal must fit 32 bits on every target, EVM included. Three existing tests, `storage_slots_instances`, `state_entry_layout_range` and `storage_slots_overflow`, used `0x100000000` (2**32) as an array length to trigger the diagnostic "layout extent overflowed". That literal is now rejected earlier ("not representable in `usize`"), so the tests no longer tested the layout check. They now use three nested arrays of length `0xffffffff` (about 2**96 slots), which overflow the slot count in the same way and give the same diagnostic with a different type text. A per-target rule would be the alternative; it was not tried.

### E.4 Master changes that matter to the layer

The Fe master the layer sits on includes the maintainer's pointer work as pull request 1892, native tail calls (1891), native core APIs (1884) and a portability series (1883) with titles such as "Make usize casts portable and size memory in usize" and "Lower native addresses at the backend's width". The portability work is compatible with a narrower `usize` and the layer builds on it. How much of the 32-bit `usize` work master already covers was read from commit titles and from the conflict resolution, not from a line-by-line review.

---

## Appendix F: Checks and results

Everything below is observed on the rebased pair, locally, before the push, unless marked.

### F.1 Sonatina

| Check | Result |
| --- | --- |
| `cargo test -p sonatina-wasm`, backend tests | 105 passed (the earlier 83 and 22 new heap tests) |
| differential test against the native JIT | 1 passed (about 115 s) |
| documentation test (README example) | passed |
| `cargo test -p sonatina-ir -p sonatina-triple -p sonatina-parser` | all passed |
| scalarizer tests, with the new one | passed; the new one fails when the fix is removed |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p sonatina-wasm --all-targets -- -D warnings` | clean (a future-incompatibility note about a dependency is printed) |

Not run on the rebased pair: the full `sonatina-codegen` test suite and the EVM suites, only the crate's own tests, the shared crates and the scalarizer tests were run.

### F.2 Fe

| Check | Result |
| --- | --- |
| `wasm_backend` | 49 passed |
| `wasm_files` | 14 passed |
| `wasm_imports` | 20 passed |
| `wasm_manifest` | 8 passed |
| `wasm_memory` | 32 passed (the 29 earlier and 3 for the heap) |
| `wasm_reproducible` | 15 passed |
| `wasm_test_command` | 11 passed (149 tests across the seven files) |
| `examples/wasm/check.sh` | passed: 19 Node tests; 128 modules built, validated and verified against manifests; a module importing something the host lacks is refused, naming it; 20 reproducible builds; 128 modules built (the two more than before are the heap fixture); `fe test --backend wasm` 4 of 4 and the same tests native 4 of 4 |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test -p fe-codegen --all-features` | passed, including the Sonatina IR snapshots |
| `cargo test -p fe-driver --test corelib` | 16 passed |
| `cargo test -p fe-uitest --all-features --no-fail-fast` | passes: name_resolution 4, parser 53, ty 4, ty_check 8, and `semantic_access` 4 when run with `RUST_MIN_STACK=67108864`. With the default stack `semantic_access` shard 1 aborts with a stack overflow in a debug build, and so does the same target on plain master (`e572b31ac7`), so it is not caused by this work. |
| full workspace run, `cargo test --workspace --all-features --no-fail-fast` | `cargo test --workspace --all-features --no-fail-fast` was run once, before the heap was added, and took 57 minutes. It reported five failing targets. Three failed because their input was an array length above `2**32 - 1`, which is no longer a valid `usize` literal once `usize` has 32 bits on one target: the command-line "type too large" test, `layout_extent_overflow_rejects_without_panicking` in the `fe-hir` layout tests, and two language-server layout hover tests. A fourth test of the same kind, `oversized_type_size_is_rejected` in the `fe-hir` frontend target, could not run because that target aborted. All four now use a pair of 32-bit lengths with the same total (for example `[[u256; 0x80000000]; 0x10000000]` for 2**59 elements) and pass: the command-line test, 11 layout tests, 113 language-server tests, and the `fe-hir` frontend target with 170 tests. The other two failures were debug-build stack overflows, `deeply_nested_malformed_type_paths_are_resolved_once` in the `fe-hir` frontend target and shard 1 of `semantic_access`. Both abort in the same way on plain master `e572b31ac7` with the default stack, so they are not caused by this work, and both pass with `RUST_MIN_STACK=67108864`. The whole workspace was not rerun after the heap was added; after it, the seven WebAssembly test files, the example script and the Sonatina crate tests were run (F.1, F.2) |
| browser check (512 by 512 Mandelbrot kernel, headless Chrome 141, against Node) | passed. The 512 by 512 Mandelbrot kernel (fixed-point, `-O s`, one exported function and the memory, no imports) was built with the rebased compiler. In Node, all 262,144 pixels match an independent BigInt reference (0 mismatches; 45,152 pixels reach the iteration limit). The same module then ran in headless Chrome 141 (desktop, no display): the 1,048,576 bytes read back from the canvas equal the Node frame byte for byte (0 differing). The frame hash (FNV-1a-32 over the pixels) is `0x2d29649a`, the same as on the earlier base. The check was repeated with the final compiler, after the heap was added, and gave the same result; the module bytes are identical to the earlier build, as expected for a program that allocates nothing. This is the only browser check; `host.mjs` has not been run in a browser, and no phone browser has been tried. |

### F.3 Not run

A phone browser; allocation-heavy and aggregate-heavy measurements; mutation runs (planting a bug to see a test fail) for the later stages S5 and S6; the full `sonatina-codegen` suite on the rebased pair; the old bugs listed in Appendix I that were not reproduced.

---

## Appendix G: Measurements

Method and scripts are in G.6. Unless marked "earlier base", a number was measured on the rebased pair with a debug build of the compiler (module bytes do not depend on that), Node 22, no imports.

### G.1 Allocation churn: dropped blocks were not reclaimed, and now are (observed)

Before the change, with the arena that never freed. Program (the text is in G.6): an exported function allocates a `MemArray<u32>` of `size` elements, uses it, and lets it go out of scope, `rounds` times. The count of memory pages is read from the instance before and after.

| Rounds | Block size | Memory grown | Per block |
| --- | --- | --- | --- |
| 1,000 | 400 bytes | 0.4 MiB | 400 bytes |
| 100,000 | 400 bytes | 38.1 MiB | 400 bytes |
| 1,000,000 | 400 bytes | 381.4 MiB | 400 bytes |
| 100,000 | 4,000 bytes | 381.4 MiB | 4,000 bytes |
| 1,000,000 | 4,000 bytes | 3,814.7 MiB (61,053 of 65,536 pages) | 4,000 bytes |

Every block's memory stayed allocated after it was dropped, because `free` did nothing in this backend and the arena only moved up. Growth is linear in the number of allocations. The last row is within 7 percent of the 4 GiB address space; about 1.1 million such allocations would trap on a refused `memory.grow`. The maintainer's Fe stack makes owners call `free` on drop, so this mattered for ordinary programs, not only for contrived loops. A first attempt with 100,000 rounds trapped for an unrelated reason (the test program summed a loop counter and overflowed a 32-bit integer); that is a program bug and the figures above come from the corrected program.

After the change (observed, Fe `3cec28f321` pinned to Sonatina `c8ef11bcf0`, the same program and driver, debug compiler, Node 22): the memory stays at 18 pages, with no growth at all, in every row.

| Rounds | Block size | Memory grown before | Memory grown after |
| --- | --- | --- | --- |
| 1,000 | 400 bytes | 0.4 MiB | 0.0 MiB |
| 100,000 | 400 bytes | 38.1 MiB | 0.0 MiB |
| 1,000,000 | 400 bytes | 381.4 MiB | 0.0 MiB |
| 100,000 | 4,000 bytes | 381.4 MiB | 0.0 MiB |
| 1,000,000 | 4,000 bytes | 3,814.7 MiB | 0.0 MiB |

The same loops are tests (`heap_churn_stays_bounded` style tests in the Sonatina crate and `memory_does_not_grow_with_the_rounds_of_a_loop_that_allocates_and_drops` in Fe) that fail if the memory grows by more than a page between one round and 100,000 rounds.


### G.2 Module size by optimization level

Earlier base (Fe `5e7e0ad544`, Sonatina native-performance branch, 60 programs, 240 builds, all succeeded). Levels `-O 0`, `-O 1`, `-O 2`, `-O s`:

- `-O 1` and `-O 2` are usually 30 to 45 percent smaller than `-O 0` (array_loops 5,126 to 2,791 bytes; vec_arena 10,711 to 3,657; shapes 13,220 to 4,824; lines 16,790 to 6,565).
- Recursion blew up at `-O 1` and `-O 2`: the self-recursive `fibonacci` was 478 bytes at `-O 0`, 99,205 at `-O 1`, 242,609 at `-O 2` and 356 at `-O s`. The cause was the same on the native backend (the Sonatina IR of the native build was about as large), so it came from the optimizer of that Sonatina branch and not from the WebAssembly code. Read, not tested: the inliner stops treating calls as recursive once the callee is inlined into its caller, and inlines again until a growth budget stops it.
- `-O s` was smallest or within a few percent of smallest for most programs.

Rebased pair (observed): Fe `2b9ed98796` with Sonatina `87af166930`: 62 programs (the fixtures plus the manifest, files and `lines` examples), 248 builds, all succeeded. Totals in bytes over the 62 programs, earlier base then rebased pair:

| Level | Earlier base | Rebased pair | Change |
| --- | --- | --- | --- |
| `-O 0` | 138,337 | 136,615 | -1.2% |
| `-O 1` | 187,472 | 82,395 | -56.0% |
| `-O 2` | 337,269 | 81,209 | -75.9% |
| `-O s` | 82,444 | 79,573 | -3.5% |

Selected rows (bytes at `-O 0`, `-O 1`, `-O 2`, `-O s`; the earlier base in brackets where it differs at a level): fibonacci 478, 430, 430, 356 (earlier `-O 1` 99,205 and `-O 2` 242,609); frame_sizes 3,645, 1,727, 1,727, 1,498 (earlier 4,042 and 6,970); recursion_frames 2,562, 2,047, 2,047, 1,949 (earlier 2,167 and 6,244); array_loops 5,126, 2,791, 2,791, 2,851 (unchanged); vec_arena 10,711, 3,657, 3,657, 4,418 (unchanged); lines 15,618, 4,179, 3,741, 5,681 (earlier 16,790, 6,565, 6,454, 7,048).

Seven or eight programs are more than 5 percent larger at every level, all with checked integer arithmetic: powmod (+25% to +31%), collatz (+18% to +22%), isqrt (+16% to +18%), factorial (+16% to +17%), arith_unsigned (+13% to +16%), digits (+10% to +13%), trap_overflow (+9% to +11%), and at the higher levels struct_state (+7%). The likely cause is the 64-bit multiply overflow check that was changed from a division to three multiplications between the two measurements, which added about 130 bytes to a modular-exponentiation kernel when measured on its own. That attribution is a reading and was not isolated with a test.

On the rebased pair the recursion growth of the earlier base is gone (observed): the self-recursive `fibonacci` is 430 bytes at `-O 1` and at `-O 2`, against 99,205 and 242,609 on the earlier base. The Sonatina main it sits on changed the native optimization pipeline in the meantime (tail calls and other work); the commit that removed the growth was not identified.

### G.3 Run time of small kernels in Node 22

Earlier base, median of 5 timed runs, results of every level agree:

| Kernel | -O 0 | -O 1 | -O 2 | -O s |
| --- | --- | --- | --- | --- |
| fib(35) | 58.7 ms | 58.4 | 74.5 | 58.8 |
| collatz_steps, 20,000 calls | 9.8 ms | 10.5 | 10.9 | 11.5 |
| count_primes(300000) | 56.5 ms | 56.2 | 56.3 | 56.8 |
| sieve (64 flags), 200,000 calls | 36.1 ms | 21.4 | 20.3 | 19.3 |
| pow_mod, 200,000 calls (before the multiply change) | 115.6 ms | 114.3 | 114.2 | 115.2 |

Reading, not tested further: the engine re-optimizes the code itself, so the Sonatina level changes run time little on scalar kernels, except where it removes memory traffic (the sieve) or bloats the module (fib at `-O 2`).

Rebased pair (observed): not re-measured. The run times of G.3 and G.4 are from the earlier base and were not repeated on the rebased pair. The change that matters for speed is the heap: allocation now takes a free-list lookup and a block header, and the example programs and tests were not timed before and after. Programs that do not allocate are not affected.

### G.4 Comparison with the maintainer's prototype, five kernels (earlier bases)

Same Fe source for both (the fixtures with `pub` removed and a `pub fn main() -> i32` that repeats the call and counts correct results, because the prototype exports only `main`). Both compiler binaries were debug builds. Every module of both returned the right count.

Bytes (this layer / prototype) at `-O 0`, `-O 1`, `-O 2`, `-O s`:

| Kernel | -O 0 | -O 1 | -O 2 | -O s |
| --- | --- | --- | --- | --- |
| fib | 518 / 440 | 28,316 / 314 | 63,076 / 314 | 389 / 358 |
| collatz | 633 / 606 | 572 / 506 | 572 / 506 | 564 / 508 |
| primes | 794 / 592 | 496 / 411 | 496 / 411 | 679 / 469 |
| sieve | 1,371 / 1,473 | 969 / 974 | 969 / 974 | 969 / 976 |
| powmod | 781 / 735 | 611 / 635 | 611 / 635 | 623 / 637 |

Milliseconds (this layer / prototype): fib(35) 58.5 to 59.7 / 63 to 67; collatz 8.9 to 10.4 / 9.8 to 12.7; primes 57.8 to 58.2 / 58.0 to 58.3; sieve 16 to 40 / 125 to 214; powmod 56 to 113 / 31 to 65.

Two causes behind these numbers:

1. The recursive expansion is in the optimizer of the Sonatina branch used then; the prototype sat on an older Sonatina without it.
2. The 64-bit multiply overflow check. This layer divided (`(a*b)/a != b`), twice per iteration of `pow_mod`; the prototype used eight multiplications and no division. After this layer switched to three multiplications on 32-bit halves, `pow_mod` for 200,000 calls went from about 57 ms to about 25 ms (the prototype: 31 to 35 ms). Module bytes grew by about 130 for that kernel.

The sieve gap (the prototype 5 to 7 times slower) was not investigated. Hypothesis, untested: the prototype's `usize` was 64 bits, so indexing and bounds checks used `i64`.

### G.5 What was not measured

Aggregate-heavy programs, strings, records, startup and compile time of large modules, native-backend speed on the same code, and any phone engine.

### G.6 Scripts

`size.sh FE OUTDIR` prints module bytes per fixture and level; set `FE_ROOT` to a Fe checkout.

```sh
@@SIZE_SH@@
```

`time.mjs OUTDIR` takes the median of five timed runs per level and checks that the levels agree.

```js
@@TIME_MJS@@
```

The churn program and its driver:

```
@@CHURN_FE@@
```

```js
@@CHURN_MJS@@
```

---

## Appendix H: Comparison with the WebAssembly prototype on the maintainer's branch

The maintainer has a separate WebAssembly prototype: Fe branch `feat/wasm-backend` (`12e547e695`, two commits on top of 15 unmerged destruction and deallocation commits) and Sonatina branch `feat/wasm-backend` (`78185d74`, two commits on Sonatina main of that time), both on the maintainer's forks. Its Sonatina commit states it is based on the translator of an MB2 branch. This layer and the prototype therefore share an origin and both use WAFFLE. None of the prototype's commits is in the branches described here (observed with git).

The comparison below was made on earlier bases, by reading both and running some experiments. It has not been redone on the current main.

| Area | This layer | The prototype |
| --- | --- | --- |
| Foundation | optional crate, WAFFLE 0.3.3 | optional feature in `sonatina-codegen`, WAFFLE 0.3.3 |
| Target triple | `wasm32-unknown-native` | `wasm32-unknown-unknown`, with layout helpers moved into a shared `isa::native_layout` module |
| `i128` | rejected with an error naming the type | two `i64` limbs |
| `i256` | four limbs; saturating forms not translated | four limbs, including `bswap` and saturating forms |
| Stack | no mutable stack pointer: hidden stack-top parameter, static frame slots, `stack_top` global; a trap leaves nothing to repair | mutable `__stack_pointer` global; a trap leaves it moved, so the instance must be created again |
| Heap | size classes with free lists and a 16-byte header, freed blocks reused, grows with `memory.grow`; `malloc` returns null on failure; optional host allocator with `arena_mark` and `arena_release` (earlier base: a bump arena that never freed) | size-class `malloc` and `free` defined by the backend, plus `memmove`; blocks reused by size class; 1 GiB cap per allocation |
| Export signatures | integers up to 64 bits and pointers; wide values and aggregates rejected | wide values as limbs, aggregates by address |
| Fe entry rule | every non-generic `pub fn` in the root is an export; text parameters and a `Reply` result | `pub fn main() -> i32` only |
| Host imports in Fe | `extern` functions become `env` imports; `std::wasm` effects `Log`, `Clock`, `Abort` | `putchar`, `getchar`, `clock` provided by the test host for `std::io` |
| Text, handle, error contract; manifest | yes; contract version 1; manifest with hash | none |
| Reproducibility | identical bytes, 368 builds tested | not stated |
| `fe test --backend wasm` | one Node process per test with `host.mjs` | a fresh in-process `wasmi` instance per test with a fuel budget; no external runtime |
| Tests | 83 Sonatina tests plus a differential test; 146 Fe tests; 19 Node tests | 17 Sonatina tests; 13 Fe tests |
| CI | a Fe workflow | a Sonatina `wasm` job |
| Fe base | master only | master plus 15 unmerged commits for `Drop`, `core::mem::{drop, forget, needs_drop}`, deallocation, fallible allocation |

Experiments on the earlier bases (observed): the prototype's 17 Sonatina tests pass (85 s). The author's 250 generated read, write and call ordering programs, ported to the prototype's harness, pass at `-O 0`, `-O 1` and `-O 2` without any ordering workaround. The same test fails on the author's backend when it ran on WAFFLE 0.2 with the workaround switched off, and passes with it. On WAFFLE 0.3.3 the author's backend passes it without the workaround, so 0.3.3 fixes the ordering problem, and 171 lines of workaround were deleted.

What each does better, as read then: the prototype has a memory model that fits the destruction work (real `malloc` and `free`), `i128`, wider export signatures, a shared native layout module, a self-contained test runner and a Sonatina CI job. This layer has the host boundary (several exports, text parameters, replies, handles, errors, manifest, versioned contract, a hardened Node host), the verification depth (differential tests against the native JIT, generated programs, reproducibility, mutation checks, about 100 Fe fixtures), a stack discipline that survives traps, and documentation.

A merge plan that was written down at the time and not carried out: base on the prototype's Sonatina backend (for `i128`, the shared layout module, CI); port from this layer the differential and generated-program tests, the reproducibility test and the longer documentation; keep the arena only as an optional mode, or implement `arena_mark` and `arena_release` on top of a size-class allocator by freeing every block allocated since the mark; on the Fe side keep the prototype's `main` rule as the default entry and add the export package, `std::wasm`, manifest and Node host as a library-mode entry. Today the two Sonatina backends are separate implementations, and the choice of base is open.

---

## Appendix I: Known gaps and limits

### I.1 Not translated or not supported

- Floating point (no `f32` or `f64` anywhere). Floats are undecided; see K.
- `i128`; enum tags, `unit` and function types as values.
- `get_function_ptr`, `sym_addr`, `sym_size`: no indirect calls and no function pointers.
- Saturating forms at 256 bits.
- Public and external global variables.
- Exports and imports of structs, arrays, `i256`, references or `usize`; an import cannot take pointers.
- EVM built-ins in programs built for WebAssembly (`balance`, `should_revert` and others); the error names the function and the instruction.
- Ingots, workspaces, `--ingot` and `--contract` with `--backend wasm`.
- No attribute selects an import module other than `env`.

### I.2 Limits that are by design

- `core::hint::black_box` is EVM-only, as its documentation says.
- A handle that is never closed is accepted by the compiler, because ownership in Fe is affine; the host counts open handles and `host.assertNoOpenHandles()` checks.
- A stack of 1 MiB by default; a call whose frame does not fit traps. Frame slots are static, so an instruction in a loop reuses its slot.

### I.3 Memory

- Freed blocks are reused (G.1, Appendix O). What is left: the memory is never returned to the engine (it only grows), there is no `realloc`, free blocks are not merged, and a block takes up to twice its request plus a 16-byte header. A program that allocates many small blocks, frees them, and then allocates many large ones keeps the memory for both.
- The largest request is 2 GiB. A request above that, or one the engine will not grow the memory for, traps for `mem.alloc_dynamic` and returns null for an external `malloc` (which Fe's fallible allocation turns into `None`).
- A block taken from a free list is not zeroed; a block cut from the top of the heap is. Fe does not rely on zeroed memory from an allocation.
- There is no maximum memory declared; the heap can use up to 4 GiB.
- A use after free now reads another block's data. Nothing in the Fe tests does so (observed with freed blocks filled with a pattern, G.1 and F.2), but the allocator cannot detect a read. `WasmBackend::with_heap_poison` fills freed blocks so that such a bug shows up as a wrong answer in a comparison with another target.
- An error thrown by a host import leaves the frame of the calling function reserved: one frame per error (documented; not rechecked on the rebased pair).


### I.4 Performance

- `addmod` and `mulmod` on 256-bit values are bit-serial. A 256-bit division whose operands do not fit in 64 bits takes one step per bit of the dividend.
- Checked 64-bit multiplication costs three multiplications on halves. A scope that chooses wrapping arithmetic avoids this cost on the Fe side; arithmetic modes through scopes were being added on a maintainer branch at the time of writing (read from titles only).
- On the rebased pair the recursion growth of the earlier base is gone (observed): the self-recursive `fibonacci` is 430 bytes at `-O 1` and at `-O 2`, against 99,205 and 242,609 on the earlier base. The Sonatina main it sits on changed the native optimization pipeline in the meantime (tail calls and other work); the commit that removed the growth was not identified.

### I.5 Things that were suspected and not reproduced

- Equality of `DynString` was reported wrong on little-endian targets in earlier work (reproduced on the native backend, not recorded with an input). On the rebased pair, seven comparisons (equal and unequal strings of 3, 32 and 37 bytes; differing in the first byte, the last byte, only past the first 32-byte word, or only in length) all give the right answer on both the WebAssembly and native backends. Not reproduced; not ruled out. The comparison is implemented in the core library (`impl Eq for DynString`, comparing payload spans), so a defect there would affect every target.

### I.6 Open checks

the full workspace was not rerun after the heap was added (only the WebAssembly test files, the example script and the Sonatina crate); the whole `sonatina-codegen` suite and the EVM suites were not run on the rebased Sonatina branch; the frame cost after an import error was not rechecked; run times were not re-measured

Three failures were seen in a full Fe workspace run on the earlier base and never traced to the merge: a native checked-arithmetic test that timed out under load (passed alone), and two debug-build stack overflows (`deeply_nested_malformed_type_paths_are_resolved_once` in the `fe-hir` `frontend` test, and the `semantic_access` uitest, which passes with `RUST_MIN_STACK=67108864`). Whether they recur on the rebased pair is in F.2.

---

## Appendix J: Web IDL and the MB2 integration

### J.1 What this is and how it relates to this layer

The MB2 line of work goes much further on the host side than this layer does. This layer's boundary is small on purpose: scalars, text in, a tagged reply and a files contract. The MB2 branches contain a Web IDL frontend and generator, a rich-value host ABI, a JavaScript host runtime, a browser compiler worker, and WebGPU work. The two were developed separately. The author wants the Web IDL bindgen to end up as a Fe program, which needs Fe metaprogramming that has not merged; until then a Rust tool produces the bindings.

GPU appears in the MB2 work only as a browser WebGPU adapter used by shaders. The WebAssembly backend itself runs on the CPU and does not involve a GPU.

### J.2 Where it is

All links are to the author's forks. Branch heads are those at the time of writing.

| What | Link |
| --- | --- |
| Fe: the MB2 line on a recent upstream Sonatina (`7018ecfadc`) | https://github.com/micahscopes/fe/tree/feat/mb2-upstream-sonatina-20261022 |
| Sonatina: the MB2 WebAssembly translator and host support (`d5c001d8`) | https://github.com/micahscopes/sonatina/tree/feat/mb2-upstream-rebase-20261022 |
| The plan, 13 phases (`WEB_PLATFORM_IMPLEMENTATION_PLAN.md`, 575 lines) | https://github.com/micahscopes/fe/blob/feat/mb2-upstream-sonatina-20261022/WEB_PLATFORM_IMPLEMENTATION_PLAN.md |
| Web IDL bindgen crate | https://github.com/micahscopes/fe/tree/feat/mb2-upstream-sonatina-20261022/crates/webidl-bindgen |
| Host ABI, host runtime and the rich-value codec | https://github.com/micahscopes/fe/tree/feat/mb2-upstream-sonatina-20261022/crates/host-abi , `.../crates/host-runtime` , `.../crates/host-wasm-codec` |
| JavaScript host runtime and codec | https://github.com/micahscopes/fe/tree/feat/mb2-upstream-sonatina-20261022/crates/webidl-bindgen/assets/browser-runtime |
| Description of the host runtime | https://github.com/micahscopes/fe/blob/feat/mb2-upstream-sonatina-20261022/demos/shared/HOST_RUNTIME.md |
| MB2 documents (WebGPU capability surface, raster slices and others) | https://github.com/micahscopes/fe/tree/feat/mb2-upstream-sonatina-20261022/docs/mb2 |
| Web IDL consolidation (retained namespace constants) | https://github.com/micahscopes/fe/tree/codex/webgpu-webidl-consolidation |
| Demo integration, readable shader names, parameter edit contract | `feat/mb2-demo-integration-20261020`, `feat/mb2-readable-shader-names-20261022`, `feat/mb2-param-edit-contract-20261021` on the same fork |
| Sonatina side branches | `feat/mb2-shader-source-names-20261022`, `feat/mb2-demanded-bitcast-20261020` on the author's Sonatina fork |

The Fe branch also carries shader, demo and web crates, so its diff size says nothing about WebAssembly.

### J.3 What exists (from the plan's own checkpoint, read, not built or run here)

Implemented and exercised on the MB2 line:

- a versioned in-memory compiler protocol, database and facade, and a real browser compiler Worker;
- HTML5-parsed inline and external Fe loading, and content-addressed production rewriting through `fe web precompile`;
- a target-neutral `#[host_import]` compiler path;
- generic resource, callback-token, future-token, ownership and structured error runtime contracts;
- linked Web IDL interfaces, partials, mixins, namespaces, typedefs, dictionaries, unions, callbacks, constants, constructors and retained iterable, maplike and setlike forms, with deterministic host and adapter plans;
- live iterable, maplike and setlike resource protocols with explicit iterator ownership, completion, error, mutation, drop and stale-handle behaviour;
- the checked-in `fe:host-wasm-codec/v1` rich-value layout and a JavaScript plan interpreter;
- a scalar and resource `std::web` Window to Document to Element vertical;
- a host-neutral EventSource to Stream to FRP layering with affine cleanup;
- canonical-source Mandelbrot evidence across revm/EVM, WebAssembly, validated browser-profile WGSL and native/Cranelift.

The bindgen crate is about 7,000 lines in six files: `lib.rs` (4,648 lines), `adapter_plan.rs` (2,423), `host_abi.rs` (1,949), `transport_plan.rs` (1,209), `selection.rs` (671) and `main.rs` (110). It holds Web IDL assets for fetch and four WebGPU slices with provenance files, and test fixtures including an upstream standards snapshot (webref 3.81.3, DOM selection) with the list of constructs expected to be unsupported. The generated raw Fe declarations were checked in; the compiler never called the generator.

Deliberately gated, in the plan's words ("product boundaries, not aliases or simulated support"):

- safe owned rich results, until the reviewed Sonatina stack-memory candidate is present in a reachable pinned dependency;
- executable guest callback trampolines and resumable async state machines;
- rich, event and promise methods in `std::web`;
- live WebGPU execution on a host with a browser GPU adapter;
- async collection execution and raw Fe rich iterator emission;
- a complete watcher and reload server.

### J.4 What the MB2 translator has that this crate does not (read from source on 22 October bases; nothing built)

The MB2 Sonatina branch is the earliest of the three WebAssembly translators (first commit 14 May, switched to WAFFLE on 15 May); its main translator is one 138 KB file.

| Feature | MB2 | This crate |
| --- | --- | --- |
| `f32` operations and a test file; `f64` rejected ("awaits an `f64` Sonatina carrier") | yes | none |
| Typed indirect calls through tables | yes | `get_function_ptr` rejected |
| Scalar globals as WebAssembly globals | yes | globals in linear memory only |
| Import module chosen by `#[wasm_import(module = "...")]` | yes | module `env` only |
| Opt-in canonical arena, or checked last-in-first-out "canonical stack memory" with `cabi_realloc` and post-return exports; scoped host borrows | yes | bump arena plus mark and release |
| Rich host values (records, strings, lists, variants, handle tokens) | `host-wasm-codec` v1 | text in and a tagged reply |
| 128 and 256-bit integers | not seen in the file list | four-limb `i256`, deep tests |
| Differential and generated-program tests, manifest, reproducibility | fewer | yes |

The MB2 Fe lowering enters through `mir::build_wasm_runtime_package`, and the Fe it emits imports `core::Browser*` types, `core::pending::Pending` and a `std::wasm::WasmBackend` type-level stub for the `Backend` abstraction; none of these exist on current master (name search only). The Fe side of MB2 therefore cannot be a base until those features land. The parts that do not depend on them can be compared and adopted separately: the Sonatina translator pieces above, the host codec and its JavaScript runtime, and the host-interface `World` model.

### J.5 What is next, in order

1. **Keep the producer separate from its products.** What the bindgen produces (the host-interface description, the codec layouts, the JavaScript runtime) should stay separate from the program that produces it, so that only the producer is replaced later. This was a stated design consequence of the plan to make the bindgen a Fe program.
2. **Port the translator features the host boundary needs into the crate** (J.4): import modules by attribute, scalar globals, typed indirect calls through tables (callbacks and trampolines need them), then `f32`. Each needs tests in the style of `crates/wasm/tests`, and each changes the "unsupported" list in the README.
3. **Decide what the memory model should offer next**, because the plan's rich results are gated on a stack-memory candidate and the model here decides whether a checked stack (`cabi_realloc` and post-return exports) is possible. The heap now frees; a checked stack and `realloc` do not exist. See K.3.
4. **Phase 4 of the plan, the general host ABI, in its rungs**: scalars and nominal opaque resources; owned, borrowed, nullable and dropped resources; strings and byte buffers with explicit encoding and ownership; records, enums, variants, options and results; lists and typed array and buffer views; callbacks with lifetime and cancellation; async results and host exceptions. Gates for each rung: a round trip through a JavaScript host and through a non-browser fake host; malformed value rejection; resource use-after-drop, double-drop and leak tests; no forgeable public handle construction; no Web-specific names in the ABI machinery. The value model should align with the WebAssembly Component Model and WIT where that preserves semantics. This layer's contract version 1 (text in, `Reply`, files) is a subset of the first rungs and can be the first version of it.
5. **Phase 5, the comprehensive Web IDL frontend and generator, in its linker rungs**: typedefs and enums; dictionaries and partial dictionaries; mixins and `includes`; callbacks and callback interfaces; namespaces, constructors, constants and special operations; iterable, async iterable, maplike and setlike; nullable, optional and variadic types, unions, sequences, records, buffers and promises; extended attributes and the exposure graph. Deliverables: source-located diagnostics, a deterministic linked interface graph, a pinned standards snapshot with provenance, interface and feature selection, raw Fe declarations, JavaScript Web IDL adapters, dependency and exposure metadata, and generated conformance fixtures. Gates: real upstream fixtures, deterministic snapshots, correct `Window`, worker, worklet and secure-context exposure, overload and conversion conformance, unsupported constructs failing at their source location, and generated raw Fe that compiles through the generic host ABI.
6. **Phase 6, the raw DOM vertical and `std::web`**: `Window`, `Document`, `Node`, `Element`, `EventTarget`, `Event`, listener options, `AbortSignal`, console, timers and animation frames, minimal fetch. Gate: a real inline Fe program changes the DOM, installs an event listener, receives an event and proves listener removal on a consuming unsubscribe. Automatic unregister on drop becomes a gate once Fe has drop glue; JavaScript finalization may be a leak backstop and never correctness.
7. **The bindgen as a Fe program.** This needs Fe's compile-time generation. Directions recorded, none decided: write the parser and generator as a plain tool first; as a stress test, probe whether a provider can be given the Web IDL text as a declared input and run a parser of the size of a small TOML parser on the compile-time evaluator within its fuel (read from code only: providers run on the ordinary compile-time evaluator with fuel; no file-input mechanism for providers was found, and that search was not exhaustive); let a library ship its generated source; let compile-time generation tools also write library source with readable names; caching was discussed without a decision. A small TOML 1.0 parser written in Fe passes its unit tests on the EVM, native and WebAssembly backends with the integration binary of that time, which makes it a ready stress test.
8. **Phases 7 to 12 of the plan**: a hardened inline Fe loader; a production precompiler; development server integration; async, streams and FRP; the native target by Cranelift; a four-backend capstone (one canonical Mandelbrot computation executed and cross-verified on EVM, WebAssembly, browser WebGPU and native).

A planning draft written during this stream recommended a first WebAssembly release of integers, imports and one text, handle and error contract, with the high-level interfaces deferred. That is consistent with what this layer ships. It is a recommendation, not a decision.

### J.6 Design rules from the plan that constrain this layer

- The compiler must not recognize names such as `Window`, `EventTarget`, `GPUDevice`, `WebGpuBackend`, `MainThread` or `Worker`. Compiler support is limited to imports and exports, a target-neutral value and resource ABI, structured inputs, artifacts and diagnostics, and target triples, layouts and capability queries.
- Generated raw bindings may be mechanical; public `std::web` APIs must feel like Fe; arbitrary JavaScript objects must not masquerade as typed Fe values.
- The shared abstraction across backends is pure computation, not a fictional universal execution environment. WebAssembly has exports, linear memory and host imports; adapters stay visibly distinct ordinary wrappers.
- Phase 2 of the plan asks that the WebAssembly emitter check on wasm32 itself, so that the compiler can run in a browser. For this crate: `cargo check -p sonatina-wasm --lib --target wasm32-unknown-unknown` finishes without errors (observed, 27 s). The crate's library compiles for wasm32. It was not built into a browser compiler or run there; the test dependencies (`wasmtime`) are not part of the library.

---

## Appendix K: Ordered backlog

Items are ordered by what unblocks other items. Each has the reason, the dependency, where it lives, and how to tell it is done. Sizes are guesses.

**K.1 Land the Sonatina commits.** Three pull requests would fit: (1) the triple, ISA and layout parameterization with the parser change; (2) the scalarizer fix and its test, which is independent and affects every backend; (3) the `sonatina-wasm` crate. Consider a CI job that runs `cargo test -p sonatina-wasm` (the prototype had one). Done when the crate is on Sonatina main. Small to medium.

**K.2 Land the Fe commits.** After K.1, point the workspace dependencies at the merged revision (a `sonatina-wasm` line next to the other Sonatina crates) and regenerate `Cargo.lock`. Five or six pull requests would fit: the target, its lowering and the `usize` rule with the three changed tests; the command-line interface and manifest; `std::wasm` and the Node host; the tests; the workflow. Done when `examples/wasm/check.sh` passes in CI. Medium.

**K.3 The memory model, what is left.** The heap with free lists is built (Appendix O). Open: (a) `realloc`, so that a growing vector can extend a block in place or move it with fewer copies; (b) merging of free blocks, or a different structure, if the waste of a phased program (many small blocks, then many large ones) matters in practice, and a limit on the waste of a power-of-two class; (c) returning memory to the engine, which WebAssembly can only do by never having grown it, so this is a question of how much a long-lived module may take; (d) the `free_high` shortcut of the plan (skip the free-list walk in `arena_release` when nothing above the mark was freed) and geometric growth of the memory, both measured as unnecessary so far; (e) a checked stack for rich results (J.5); (f) whether a fallible allocation should be able to fail below 2 GiB for programs that want a lower limit. Done when a measurement shows the item is needed. Small to medium each.


**K.4 The `usize` rule.** Decide between "literals must fit 32 bits on every target" and a per-target rule. The first is simpler and keeps casts lossless everywhere; its cost is E.3. The portability series on master (E.4) is the place to settle it. Small, but a language decision.

**K.5 The optimization pipeline for wasm32.** Choose native or size-oriented. `-O s` was smallest in most programs and the engine re-optimizes anyway (G.3). Re-measure first (G.2, G.3). On the rebased pair the recursion growth of the earlier base is gone (observed): the self-recursive `fibonacci` is 430 bytes at `-O 1` and at `-O 2`, against 99,205 and 242,609 on the earlier base. The Sonatina main it sits on changed the native optimization pipeline in the meantime (tail calls and other work); the commit that removed the growth was not identified. Small.

**K.6 Checks that were not run** (F.3): the full Sonatina suites on the rebased pair, aggregate-heavy measurements, mutation runs for stages S5 and S6, a phone browser, a test for the frame cost after an import error, a browser run of `host.mjs` (it has only run in Node; one kernel ran in headless Chrome). Small each.

**K.7 Wider integers and operations.** `i128` and the saturating forms at 128 and 256 bits (the prototype has them); faster `addmod` and `mulmod` (the techniques of the native 256-bit division on Sonatina main could be reused); the saturating 256-bit forms. Medium.

**K.8 The `Backend` port.** The Fe `Backend` abstraction was being planned in another stream; WebAssembly would implement it once it exists. This layer does not depend on it. Keep the host boundary (D.3) behind a small seam so the port is a rewiring. Not started here.

**K.9 Floating point.** Undecided: a primitive type, or a library over bits. The MB2 translator has `f32` (J.4). If a primitive type: it needs type-level work in Fe, float types in the Sonatina IR as far as the backend sees them, and translation for each float operation; `f64` needs a carrier. Medium to large, and a language decision first.

**K.10 Rich host values and Web IDL.** Appendix J.5. Large; depends on K.3 and on Fe metaprogramming for the final form.

---

## Appendix L: Decisions

Taken by the author, as recorded during the work.

| Date | Decision |
| --- | --- |
| 2026-10-26 | The memory model is expected to change substantially; comparisons that depend on it (stack, heap) are provisional. |
| 2026-10-26 | The Web IDL bindgen should eventually be a Fe program; until then the Rust tool is a stopgap. |
| 2026-10-27 | WebAssembly is a third compilation target with a 32-bit `usize`. |
| 2026-10-27 | The merged line is the only line to develop and publish; earlier branches are records and are not deleted or force-pushed. |
| 2026-10-27 | Rebase onto the maintainer's native backend and carry over the host boundary, tests and documentation. |
| 2026-10-27 | The layer stays self-contained and easy to rebase: it does not depend on the code-generation, suspension or `Backend` layers unless a feature needs it. The integration branch is a throwaway sketch and is secondary. |
| 2026-10-30 | Older WebAssembly, layer and integration branches are pushed as records under `wip/` names; branches that add text which cannot be published are held back. |
| 2026-10-30 | The layer is re-applied onto current master and main, not merged. |
| 2026-10-30 | A heap that reuses freed memory is built before the handoff, after a review of a design that was prepared from the code (Appendix O). |
| 2026-10-30 | The rebased branches are pushed under new names on the author's forks, with Sonatina first and Fe pinned to its revision. |
| 2026-10-30 | The prototype on the maintainer's branch is not merged in; it stays a separate line (Appendix H). |

Still open: floating point; the remaining items of the memory model (K.3); the `usize` rule; the pipeline; how to land.

---

## Appendix M: Glossary

- **Arena**: the region of linear memory after the stack, from which `mem.alloc_dynamic` allocates by moving a top pointer up.
- **Backend**: in Sonatina, an implementation that turns a module into output (`Backend` trait). In Fe, a planned abstraction over targets, not yet present.
- **Canonical (narrow integer)**: an `i1`, `i8`, `i16` or `i32` held zero-extended in a WebAssembly `i32`.
- **Differential test**: running a generated program on two backends and comparing the results.
- **Export, import**: a function the module offers to the host, or one it needs from the host.
- **Frame**: the stack space of one call, with a static slot for each local aggregate.
- **Host**: the program that instantiates a module and supplies its imports (Node, a browser, another engine).
- **HostText**: the type of text that the host placed in the module's memory, valid for one call.
- **Layer**: a separable body of work on a branch of its own, kept loose so it can be rebased or merged alone.
- **Limb**: one 64-bit piece of a 256-bit integer.
- **Line**: the chain of branches that carries one body of work.
- **Manifest**: the JSON file written beside a module that describes its imports, exports, hash and the host contract version.
- **Record (branch)**: a branch kept for history, not for development.
- **Reply**: the `i64` result type of an export that returns a tag and a payload.
- **Target**: the Fe compilation target (`Evm`, `Native`, `Wasm`), not to be confused with the Sonatina triple.
- **Trap**: a WebAssembly runtime error that ends the call (`unreachable`, a refused `memory.grow`, an out-of-range access).
- **WAFFLE**: the Rust crate that recovers structured control flow and encodes WebAssembly, used by the translator.

---

## Appendix N: Revisions and provenance

### N.1 Revisions

| What | Revision |
| --- | --- |
| Rebased Sonatina branch | `c8ef11bcf04fb48ba4be381c4855927dbea0f649` on main `c30e6b22e57ab5315e634539a5e072ee41c67b17` |
| Rebased Fe branch | `3cec28f321a159a2e206748d14c4d20f221e5c1a` on master `e572b31ac72231b5d740ffed1b551ee63f360e8b` |
| Older merged line, Fe | `5e7e0ad54484b3092c74a13bc6e7100597a48320` |
| Older merged line, Sonatina | `eae597bcfcc3ef506542562b7b780def01b50158` |
| Pointer-address branch the older line was built on (Fe) | `c965857c32738a3b9c8b7e47bc63dd206157208f` |
| Native-performance head the older line was built on (Sonatina) | `bbbcafe3` |
| Prototype on the maintainer's branch | Fe `12e547e695`, Sonatina `78185d74` |
| MB2 line | Fe `7018ecfadc`, Sonatina `d5c001d8` |

### N.2 Where each number came from

- Test counts and the check-script output: the commands in B.2 and B.3, run on the rebased pair locally before the push.
- The planted-bug checks: the scalarizer condition replaced by `true` (one line changed, the new test fails); the earlier carry-term check for the 64-bit multiply.
- Size and time numbers: the scripts in G.6 with the binary of the revision named in the section.
- Allocation churn: G.6.
- The count of commits by author on the older lines: `git log` over the range with the author's email.
- The comparison with the prototype: earlier bases, source reading plus the experiments listed in Appendix H.
- The Web IDL appendix: the documents and source of the MB2 branches, read; nothing in it was built or run here.

### N.3 Things stated without a test

How much of the 32-bit `usize` work master already covers (commit titles and the conflict resolutions only). The cause of the inliner growth (a reading). Whether `host.mjs` and the modules run on a phone browser. Whether the MB2 branches build today.

---

## Appendix O: The freeing allocator, its design and what was built

This is the design of the allocator that replaces the never-freed arena by one that reuses freed blocks, followed by what was built (O.7). It was prepared by reading the crate, the maintainer's allocator and the Fe lowering, then reviewed by an independent read of the same code that was not given the first design. Statements marked (read) were verified in code by that review or by the author; (inferred) were reasoned out. Sections O.1 to O.6 are written in the future tense of the plan; O.7 says what happened.

### O.1 Why

G.1 measures the leak. Fe master destroys owned values and calls `free` on native; on WebAssembly `free` is skipped and every dropped block stays allocated. The leak also hides any Fe bug that frees too early (O.6, risk 2).

One correction to the description in the rest of this document: `arena_release` already reclaims memory for calls that use the host-allocator protocol. In `examples/wasm/host.mjs` only a call with a text argument marks and releases; scalar exports do not. So a text export is bounded between calls, though not within one call, and the leak above is the scalar-export or within-one-call case.

### O.2 Decisions

**A. Structure: power-of-two size classes, last-in-first-out free lists, a 16-byte header, no splitting and no merging.**
- Class k holds 2^k payload bytes for k = 4 to 31. A request goes to class `32 - clz(max(size, 16) - 1)`, as in the maintainer's `malloc_body` (`crates/codegen/src/isa/wasm/runtime.rs` on his Sonatina branch) (read).
- Free lists are linked through the first word of the payload. A freed block goes on the front of its list, which is deterministic.
- The 32 list heads live in a 128-byte table in linear memory after the global variables and before the stack. They cannot be WebAssembly globals because the head is chosen by class index at run time. The global variables stay at address 16, which an existing test asserts (read).
- Header, 16 bytes so that payloads stay 16-byte aligned: word 0 is the class; word 1 is a state (LIVE, FREE, or 0 for never handed out or cleared); words 2 and 3 are unused (later: the requested size).
- Blocks are cut only from the top of the heap and never split or merged. So every block lies wholly below or wholly above any mark, since a mark is a past value of the top. O.2 C depends on this. A merging allocator could build a free block that crosses a mark; that is the main reason to prefer size classes over the alternatives.
- Cost: a block takes `2^class + 16` bytes, so waste is under 50 percent plus 16 bytes (400 bytes takes 528; 4,000 takes 4,112; 16 takes 32). Without merging the heap can grow to the sum of each class's peak use; this is bounded and deterministic.
- Largest allocation 2^31 bytes (an intended change; today an allocation can use nearly 4 GiB).
- When memory cannot grow, `malloc` returns null without changing anything. `mem.alloc_dynamic` calls `malloc` and traps on null, so the instance stays usable as documented. The host `alloc` export still traps. An external `malloc` returns null, as C's does.
- "The end would pass 2^32" must be tested as `end >> 32 != 0`. The maintainer's version tests `end > 1 << 32`; with `end == 2^32` exactly the top wraps to 0 and the next block starts at address 0 (read in his `malloc_body`). Do not copy that.

**B. Zeroing: drop it for blocks the module allocates; keep it for the host `alloc` export.**
- Nothing relies on module allocations being zero. The core library says `alloc_raw` "establishes a byte extent, not initialized typed contents"; `MemArray::new_zeroed` and `try_new_zeroed` zero the memory themselves (`ingots/core/src/ptr.fe`) (read). The native backend implements `mem.alloc_dynamic` with the host's `malloc`, which does not zero, and Fe's wasm tests compare against native, so Fe already works with dirty memory (read). No Sonatina optimizer assumes `mem.alloc_dynamic` memory is zero (searched `optim/` and `transform/`).
- Host `alloc` stays zeroed at no cost, given the invariant that every byte at or above the top is zero: fresh pages are zero and `arena_release` clears `[mark, top)`. Host `alloc` then only cuts from the top and never takes from a list.
- Tests that assume zero change: `arena_blocks_are_aligned_zeroed_and_never_overlap` becomes "zero when freshly cut, disjoint while live".

**C. `arena_mark` and `arena_release` with free lists.**
- Constraint: `arena_mark` must stay free of side effects. The test driver (`crates/fe/tests/wasm_support/files_driver.mjs`) calls it after every call without releasing, and `the_arena_is_released_after_every_call` asserts the value never changes (read). That rules out a mark that saves state.
- What goes wrong if release only moves the top back: (1) list entries left above the new top, so the next fresh cut overlaps a block that is still on a list; (2) chains that cross the mark, which clearing `[mark, top)` cuts in the middle; (3) a host block taken from a free list below the mark, which release would never free; (4) a module block taken from a list after the mark and still live at release; (5) blocks allocated before the mark and freed during the call, which must stay on the lists (emptying all lists at release would make the top grow without bound when scalar and text calls alternate); (6) nested marks, when an import calls back into an export that takes text; (7) a mark that `arena_mark` never returned, inside a block.
- Rule. `alloc` (host): cuts only from the top, traps on failure, returns zeroed memory, block format as module blocks so module `free` works on one (Fe never frees one: `HostText` is `Copy` and has no `Drop`). `arena_mark`: unchanged. `arena_release(m)`: check `m` as today and trap without changes otherwise; walk every class list once and unlink each node whose block does not end at or below `m`, keeping the order of survivors; check every visited node (aligned, inside the heap, header state FREE) and trap otherwise, which also stops a loop made by use-after-free damage; clear `[m, top)`; set the top to `m`. The walk must come before the clear because surviving chains run through nodes above the mark.
- Cost: the walk is proportional to the number of free blocks, per release, which usually averages one step per `free`. The bad case is a large set of freed blocks below the mark left by scalar calls. An optional global `free_high`, the highest address freed since the last walk, lets release skip the walk when `free_high < m`.
- Contract change: "release frees every block allocated after the mark" becomes "release frees every block at or above the mark: all host blocks, and module blocks that were cut from the top". Module blocks taken from free lists after the mark and never freed stay allocated, as on native.

**D. How Fe's `free` reaches the module's `free`: intercept external declarations, as is done for `memmove`.**
- In the crate, generalize `is_memory_copy_declaration` (`src/translate.rs`) into a classifier returning a call target: `MemoryCopy` as today; `Malloc` for `malloc(i32 or i64) -> ptr` (an `i64` size over 32 bits returns null); `Free` for `free(ptr) -> ()`. A call to either becomes a direct call to the module's own function: no stack argument, not an import.
- An external `malloc` or `free` whose signature does not match is an error naming the expected signature (the maintainer's `check_signature` does this). Today an external `free` becomes an `env.free` import; that changes on purpose.
- `scan_module` must see these declarations because memory planning runs before the import loop; they set the heap and runtime flags. A module that only uses `memmove` must not get a heap. Without the scan fix there is a silent failure: the `if let (Some((func, sig)), Some(globals))` in `translate.rs` does nothing when `globals` is `None`, and the allocator keeps its empty placeholder body. Make that an `expect`.
- In Fe (`crates/codegen/src/sonatina/lower_runtime.rs`): `RuntimeBuiltin::Free` changes `if self.module.has_host_libc()` to `is_native_target()`; `lower_native_allocation` changes `fallible && self.module.has_host_libc()` to `fallible && self.module.is_native_target()`; `has_host_libc` then has no callers and goes; the comments at both places ("nor is the memory arena of a WebAssembly module", "as does every allocation of a WebAssembly module") are fixed. Infallible `Malloc` stays `MemAllocDynamic` because the backend sends it and external `malloc` to the same heap.
- `NATIVE_EMPTY_ALLOCATION` is 16 and its comment says it lies in an unmapped first page. On WebAssembly address 16 is `DATA_BASE`, the address of the first global (read). It still works because `free` checks for 16 and the heap never returns 16. Fix the comment, or reserve address 16.
- Rejected: a new `mem.free_dynamic` IR instruction (touches IR, parser, verifier, interpreter, Cranelift, EVM legalization and the wasm crate; Fe's native path already uses an external `free`); a compiler-private symbol (no gain over the C name Fe already reserves).

**E. `TryMalloc` returns null on WebAssembly: yes.** Fe handles it: `try_alloc_bytes` checks `ptr as usize == 0` and returns `None` (`ingots/core/src/ptr.fe`). `MemArray::try_new_*` and the `try_alloc_bytes` users return `None` instead of trapping. A request above 2 GiB now fails without touching memory. No Fe wasm test expects a trap from a fallible allocation (searched).

**F. Other effects.**
1. Use-after-free and double free become visible. Today nothing is reused, so a Fe drop that frees too early is harmless; with reuse it corrupts memory silently. The header states catch a double free and a free of a non-block. Add a test-only `WasmBackend::with_heap_poison()` that fills freed payloads with a pattern, so wasm-against-native comparisons turn a use-after-free into a wrong answer.
2. Tests that assume adjacent blocks in increasing order change: `the_host_allocator_places_data_that_the_module_reads`, `exhaustion_traps_and_corrupts_nothing`, the release test (`mark == kept + 112`), and `arena_blocks_...`. The documented meaning of `arena_mark`, "the address the next block gets", becomes "the top; the next host block's payload is at mark + 16". `host.mjs` does not rely on that equality (read).
3. `malloc`, `free` and host `alloc` are leaf functions without frames. They take no stack argument and do not touch `stack_top`, and call no imports, so a host callback cannot interrupt them. To keep "a trap changes nothing", every check comes before the first write.
4. Determinism: output bytes are unaffected; run-time addresses depend only on the sequence of operations, so tests can compare exact addresses with a Rust reference model.
5. Module size: the allocator is emitted only when the scan finds `mem.alloc_dynamic`, `malloc` or `free`, or `with_host_allocator`; other modules stay byte-identical. Modules with a heap grow by an estimated 400 to 700 bytes (inferred).
6. Layout: the list table pushes `stack_base` up by 128 bytes in modules with a heap; the stack is rounded to a page, so the initial page count is unchanged unless the globals are near 64 KiB (inferred). The module documentation lists the regions and gets an "allocator state" entry.
7. Out-of-date text to fix: `memory.rs` says the arena holds "objects that outlive their frame", and `translate/aggregate.rs` says it serves only `mem.alloc.dynamic`.
8. `memory.grow` happens once per 64 KiB of fresh cuts. Fine for now; geometric growth is optional.
9. Host `alloc` and `mem.alloc_dynamic` are one function today and become two.

### O.3 Intentional changes to documented contracts

Module blocks are no longer zeroed. Every block has a 16-byte header, so block address arithmetic in tests changes. The largest allocation drops to 2 GiB. `arena_release` no longer frees module blocks taken from free lists after the mark. An external `malloc` or `free` stops being a host import, and a mismatched one is an error. External `malloc` follows C and returns null on failure, while `mem.alloc_dynamic` and the host `alloc` keep trapping.

### O.4 Build plan, in commits

Sonatina (crate `crates/wasm`):

- **S1, refactor with no behaviour change.** Rename `arena_top` to `heap_top`; add a `HeapFuncs { malloc, free, host_alloc }` placeholder where `malloc` is today's arena function; replace `is_memory_copy_declaration` with `runtime_call_target`; add `Scan.heap`; turn the silent skip into an `expect`. Test: the existing suite passes unchanged.
- **S2, block format, `malloc` returning null, and host `alloc` cutting from the top.** `memory.rs`: the list table in `plan_memory`, `build_malloc` (list pop, or cut from the top with growth, null on failure, header written after all checks), `build_host_alloc`. `translate/aggregate.rs`: `arena_alloc` becomes a call to `malloc` plus an inline null trap. README: rewrite "The arena" and the host table. Tests: update the four address-arithmetic tests; add `blocks_round_to_a_power_of_two_class_behind_a_header`, `allocation_above_two_gib_fails_without_growing`, and the refused-growth test adapted.
- **S3, intercept `malloc` and `free`, the checked `free` body, and the release walk.** One commit, because `free` without the walk breaks modules that use the host allocator; to split it, reject `free` together with `with_host_allocator` first and lift that in the next commit. Tests: `external_malloc_and_free_are_module_functions_and_not_imports`, `a_mismatched_external_free_is_an_error`, `free_of_null_does_nothing`, `double_free_and_free_of_a_non_block_trap_and_change_nothing`, `a_module_that_only_frees_has_a_heap`, `try_malloc_returns_null_when_memory_is_refused`, and the mark and release interplay tests: `blocks_freed_above_the_mark_are_forgotten_by_release`, `blocks_allocated_before_the_mark_and_freed_after_it_are_reused_after_release`, `a_free_list_chain_crossing_the_mark_keeps_its_low_part`, `nested_marks_release_innermost_first`, `a_mark_inside_a_block_does_not_corrupt_the_free_lists`, `host_alloc_never_reuses_a_listed_block_and_returns_zeroes`.
- **S4, property and churn tests** (new `tests/wasm/heap_model.rs`). `random_alloc_free_mark_release_sequences_match_a_reference_heap`: a module exporting allocation through `alloc_dynamic`, through external `malloc`, `free`, and the host `alloc`, `arena_mark` and `arena_release`; a Rust reference with the same classes, LIFO order and release rule; about 2,000 operations per seed, several seeds, at `-O 0` and `-O 2`; sizes skewed toward 0, 1, 15, 16, 17, 2^k plus or minus 1 and up to 200 KB; every new block filled with its own pattern. After each step check exact addresses against the model, pairwise disjoint live blocks with their patterns intact, 16-byte alignment, zeroed host blocks, and the top bounded by the sum over classes of peak live count times `2^k + 16`. One variant uses a small memory limit so null and trap paths occur mid-sequence, and a failed step must leave the model unchanged. Also `a_churn_loop_stays_bounded` (memory size after 100,000 rounds equals memory size after one round, at 400 and 4,000 bytes and with three sizes in rotation) and `churn_between_text_calls_stays_bounded`.
- **S5, optional.** The `free_high` shortcut, `with_heap_poison()`, geometric growth.

Fe, after S3:

- **F1.** The two condition changes, delete `has_host_libc`, fix the comments, update `examples/wasm/README.md` ("never frees") and the `try_alloc_raw` documentation in `ptr.fe`.
- **F2.** Guards that must still pass: `vec_arena`, the assertion in `wasm_files.rs` that the imports of the mock are empty (it proves `free` and `malloc` are not imported), and `the_arena_is_released_after_every_call`. New fixture `heap_churn.fe` with `churn(rounds, n)` (a `MemArray<u8>::new_zeroed(n)` per round), `vec_churn(rounds)`, and `try_huge()` (`MemArray::try_new_uninit(3_000_000_000)` returns 0 for `None`). New tests `heap_churn_stays_bounded` (memory size before and after 100,000 rounds at all four levels) and `try_allocation_is_none_on_wasm`, and the native comparison extended to the new fixture.

### O.5 Size

Roughly: `memory.rs` 550 to 700 lines; wiring in `translate.rs` and `aggregate.rs` 200 to 250; README 60; Sonatina tests about 750 (100 updated, 300 for S3, 350 for S4); Fe lowering about 25 lines; Fe docs 20; Fe tests about 150. Parallel work is possible: the S4 model and harness can be written alongside S2 and S3 against the agreed policy; F1 and the Fe fixtures can be written alongside but cannot pass until S3 lands (before then `free` becomes an import and instantiation without imports fails); documentation can go alongside.

### O.6 The three biggest risks

1. **Free-list corruption across mark and release.** It shows up as occasional overlaps much later. Write the reference-model property test first (it can be written against S2's `malloc`) and run it before S3 lands; keep the release walk's node checks, which trap rather than corrupt; pin the hand-written chain-crossing test.
2. **Hidden use-after-free and double free in Fe's drop lowering,** which the current leak hides. The header-state traps catch some. Run the whole Fe wasm suite (`wasm_memory.rs`, `wasm_files.rs`, all four levels) with `with_heap_poison()` on, as soon as F1 compiles. A difference from the native build is a Fe bug and not an allocator bug.
3. **Wiring holes in the scan and the plan.** A module with a heap but no runtime gets a function with an empty body (silent). An unrecognized `free` declaration becomes an `env.free` import that fails at instantiation. Catch them with the `expect` in S1, `a_module_that_only_frees_has_a_heap` in S3, and the no-imports assertion in Fe.

A smaller risk is fragmentation from the lack of merging. Detect it with a phased-size property run (many small blocks freed, then many large) that checks the growth bound.

### O.7 What was built, and where it differs from the plan

Built, in the Sonatina crate (commits 4 and 5 of A.1) and in Fe (commits 8 and 9):

- The heap of O.2 A, B, C and D: size classes 4 to 31, a 16-byte header with class and state, last-in-first-out free lists in a 128-byte table, blocks cut only from the top, never split or merged; `malloc` returns null and `mem.alloc_dynamic` traps; `free` checks the header and traps on a double free or a non-block; the host `alloc` cuts zeroed blocks from the top; `arena_release` walks the lists, checks every node with a bound on the number of nodes, unlinks what ends above the mark, clears and moves the top back.
- The interception of the external `malloc` and `free` as module functions, with the scan so that a module that only calls `free` still gets a heap, and an error for another signature.
- In Fe, the two conditions in `lower_runtime.rs`, the removal of `has_host_libc`, and the documentation; a fixture and tests that a loop of allocate and drop stays bounded at all four levels, that it agrees with a Rust reference and the native build, and that a fallible allocation that cannot be met is `None`.
- The `with_heap_poison` option (S5 in part).

Differences from the plan:

- The external `malloc` accepts a 32-bit size only. The plan allowed a 64-bit size that returns null above 32 bits. Fe uses the width of `usize`, which is 32 bits on this target, so a 64-bit declaration is an error that names the signature.
- The refactor commit S1 was folded into the allocator commit, and the interception and the release walk were not split from it (the plan allowed either).
- The `free_high` shortcut for `arena_release` and geometric growth of the memory (S5) were not built. No measurement showed a need.
- `NATIVE_EMPTY_ALLOCATION` stays 16 and its comment now says what that address is in a module, where the plan allowed reserving address 16 in the layout.
- The Fe README bullet that listed the equality of `DynString` and `Bytes` as a known defect was removed, because the comparison gives the right answer (I.5).
- The model test uses five kinds of operation (allocate through `malloc`, through `mem.alloc_dynamic`, through the host `alloc`, free, mark and release) and compares every address with the model, but it does not set a memory limit; a second test uses a small memory limit and checks that failures change nothing.

Observed results:

- Memory growth of the churn loop of G.1: 0.0 MiB in every row, against up to 3,814.7 MiB.
- Sonatina: 105 backend tests, the native-JIT differential test and the documentation test pass; formatting, lints as errors and a compile for wasm32 are clean.
- Four planted bugs, one at a time (the release walk never unlinks, the release walk unlinks every node, `free` does not check the state, a reused block is not removed from its list), each made between 1 and 6 of the heap tests fail. Each run first confirmed that the mutation changed two lines.
- Risk 2 of O.6: the seven Fe WebAssembly test files were run with freed blocks filled with a pattern, through a temporary environment switch that was not committed. All tests passed, except two reproducibility tests that failed because their two builds differed in the 19 bytes of the fill instruction (the switch was set for one build and not the other). No program in the suite reads a block after freeing it.
- Risk 1 of O.6: the random-sequence test against the reference heap passes at two optimization levels and four seeds; the chain-crossing test passes in four free orders.
