# Prototype: `fe test --locate-failures`

Local prototype, not for merge as written. It explains a failed `fe test` case with the Fe
source location of the failing instruction and the call stack that reached it, using the
trace facts and ethdebug-style data that `fe dev trace emit` already produces.

Branch: `local/ethdebug-20261026`, based on `argot/master` at `e8612c21da`. Nothing is pushed.

## Example

```
$ fe test . -O 0 --locate-failures
PASS  [0.0046s] total_in_range
FAIL  [0.0053s] total_out_of_range_fails_in_other_module
    Test reverted: 0x4e487b71...0001
    REVERT inside `assert` (builtin-std:/src/evm/effects.fe:1232:5), reached from src/math.fe:2:5  `assert(x < 100)`
    call stack, innermost first:
      assert  builtin-std:/src/evm/effects.fe:1232:5
      scale  src/math.fe:2:5  `assert(x < 100)`
      scaled_total  src/math.fe:8:5  `scale(sum)`
      total_out_of_range_fails_in_other_module  src/lib.fe:10:12  `scaled_total(60, 50)`
```

Without the flag the output is unchanged: only the first line (`Test reverted: ...`) is printed.

## Run it

```
cargo build -p fe --no-default-features
target/debug/fe test <file-or-ingot> -O 0 --locate-failures
```

`-O 0` is recommended. At the default level (`-O 1`) the optimizer can fold a test that uses
only constants into one unconditional revert, and then no Fe source remains to point at; the
output says so instead of guessing.

Regression tests (single file, two-module ingot, folded test, unexpected revert reason):

```
cargo test -p fe --no-default-features --test locate_failures
```

## How it works

1. `fe test` compiles and runs each test as before.
2. With the flag, the test module is compiled a second time with trace facts
   (`codegen::trace::emit_observable_test_trace_facts`). This reuses the contract trace path:
   `emit_runtime_package_sonatina_bytecode_with_trace` and the `DebugBundle` from `debug-export`.
3. The traced init code of each test must equal the bytecode that was executed, byte for byte.
   If it does not, the test prints `source location unavailable` and no location.
4. When a test fails, the call is replayed on a cloned EVM context
   (`RuntimeInstance::call_raw_pc_path`) to collect the program counters of the outermost frame.
5. `crates/fe/src/test/locate.rs` maps the last program counter to an exact source attribution,
   and rebuilds a call stack: a `JUMP` that lands on the entry of a compiled function is a call,
   a `JUMP` back into the middle of another function is a return.

Passing tests are not replayed. The second compile is paid for every test in the run when the
flag is given, including when nothing fails.

## What was tested

Everything below ran on the calc machine (Linux, x86_64), in a debug build with
`--no-default-features`, through the in-process revm harness that `fe test` uses.

- The 7 tests in `crates/fe/tests/locate_failures.rs` pass.
- Existing tests that touch the changed code pass: 26 tests in the `fe` binary matching
  `trace`, `debug`, `locate` and `test::`, and `trace_semantic_canary` in `fe-codegen`.
- `cargo fmt --all -- --check` is clean. `cargo clippy --locked -p fe -p fe-codegen
  -p fe-contract-harness --no-default-features --all-targets -- -D clippy::all` is clean.
- Wall time with and without the flag, one run each, debug build, busy machine:
  `abs.fe` (27 tests) 6.6 s and 7.7 s; `array_ops.fe` (6 tests) 4.8 s and 4.9 s. Indicative only.

Not run: the full workspace test suite, the CI-style all-features build, a release build,
macOS or Windows, the default features (language server, doc server), and the `--backend`
variants other than the default Sonatina one.

## Limits

- Only the outermost frame is followed. A failure inside a contract that the test called is
  reported at the call in the test, not inside the called contract.
- No values or local variables are shown. An assertion reports where it failed, not the operands.
- The stack is made of compiled functions. Functions the optimizer inlined have no frame.
- Library code (`builtin-std:` and `builtin-core:`) is reported by path and position only; its
  text is not on disk, so no snippet is printed.
- Only the last 65,536 executed steps are kept; when a run is longer, the output says outer
  frames may be missing.
- The revert payload is still printed raw (for example the `Panic(uint256)` selector and code).
- `fe dev trace emit --tests` was added to inspect the facts of a test module; it is a
  development aid and accepts one standalone file.
