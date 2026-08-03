# Reviewed Sonatina browser-backend overlay

The first 28 `git format-patch` files reproduce the exact unpublished Sonatina
commit `ac266c210cad7872fc98380a73b4ca363877bc1f` from the remotely fetchable Fe
workspace base `150d327edfa88374802a6cc8089fd77da5fa818b`.
Patches `0029`–`0032` are the next reviewed upstream candidates, authored
in a dedicated worktree beginning at Fe's pinned
`43e9f3b0d60fff4f8f7006174b9f1d406a0c70f0`. They apply cleanly after `0028`,
but are intentionally not wired into Fe's Cargo pin until an upstream commit
containing them is remotely reachable.

The series is committed evidence for the f32 IR/Wasm/SPIR-V substrate, structured
shader control flow, typed shader inputs, scalar comparisons and shifts, and the
opt-in canonical Wasm arena used by the browser demos. In particular:

- `0001`–`0021` provide the browser-profile f32 and SPIR-V path;
- `0022` adds `WasmBackend::with_canonical_arena`;
- `0023`–`0025` add typed scalar memory, dynamic arena allocation, and bitwise
  lowering needed by canonical actor Wasm;
- `0026` lowers narrow integer truncation according to its source and target
  carriers;
- `0027` lowers signed and unsigned integer extension across Wasm carriers.
- `0028` keeps private helper functions callable but out of the Wasm host ABI.
- `0029` adds an opt-in checked LIFO canonical-memory manifest. It emits
  `cabi_realloc` plus identity-scoped post-return exports for generator-owned
  result stacks, preserves default Wasm bytes, and traps malformed, stale,
  out-of-order, double-free, and out-of-bounds cleanup. It is deliberately not
  a general-purpose allocator: resize and cleanup must follow stack order.
- `0030` lowers scalar Sonatina globals to genuine Wasm globals. Typed
  `mload`/`mstore` operations on global values become `global.get`/`global.set`,
  preserving initializers and per-instance persistent state without consuming
  the canonical allocator's linear-memory prefix.
- `0031` adds a target-neutral typed indirect-call IR operation. It consumes a
  function-pointer value plus an explicit pointer-to-function signature,
  verifies argument and result types, interprets `GetFunctionPtr` values,
  conservatively models effects and address-taken call-graph edges, and keeps
  Wasm/Cranelift lowering fail-closed until real table lowering is added.
- `0032` lowers that typed operation to WebAssembly tables. Address-taken
  functions receive deterministic non-zero slots, slot zero remains null,
  function-pointer ABI values use the Wasm `i32` table-index carrier, and
  `call_indirect` retains Wasm's null, out-of-bounds, and signature traps while
  preserving multi-result order.

This remains a reviewed patch archive rather than a workspace dependency
mutation. [`demos/with-sonatina-overlay.sh`](../../../demos/with-sonatina-overlay.sh)
is an explicit, command-scoped consumer: it verifies every entry in
`SHA256SUMS`, reconstructs the series in an isolated checkout, validates the
reviewed commit IDs, and removes the checkout after the command. It never edits
the workspace manifest, lockfile, Fe's declared Sonatina pin, or a
caller-supplied checkout.

The series has an intentional bridge. Patches `0001`–`0028` reconstruct
`ac266c21` from `150d327e`; Fe's exact `43e9f3b0` pin descends from that commit
and includes two subsequently published scalar-op commits. The runner validates
the historical reconstruction, checks out that exact pin in the isolated
checkout, then applies `0029`–`0032` and validates `548b7e54`, `7ced9661`,
`09e2895e`, and `a170ef04` in order.

Use it with Cargo directly:

```sh
demos/with-sonatina-overlay.sh cargo test -p fe-codegen
```

For a Cargo command, the runner injects command-line `[patch]` overrides for
`sonatina-ir`, `sonatina-triple`, `sonatina-codegen`, and
`sonatina-verifier` and runs Cargo from an isolated source snapshot, so
path-resolution changes cannot rewrite the caller's `Cargo.lock`. For a wrapper
command it exports the temporary
`SONATINA_DIR`; existing demo generators use that path for the same four
overrides. `SONATINA_DIR=/path/to/clean/sonatina` supplies an offline object
source without being modified. Otherwise the runner uses
`FE_BROWSER_CACHE_DIR` (default `target/fe-browser-cache`) and serializes cache
population; `FE_BROWSER_OFFLINE=1` rejects cache misses. `FE_DEMO_TMPDIR`
relocates disposable overlay checkouts.

This is sufficient for a callback integration proof without claiming that the
published pin has the feature: run the proof's Cargo test through the wrapper,
lower an address-taken Fe callback to the typed Sonatina `call_indirect` IR, and
execute the resulting Wasm through the host callback registry. The proof can
assert the returned value plus null, out-of-bounds, and signature-mismatch
traps, while build logs retain the validated `a170ef04` overlay identity.

The checked-in scalar callback capstone is:

```sh
demos/with-sonatina-overlay.sh cargo test -p fe-codegen \
  --features sonatina-indirect-calls \
  overlay_wasm_guest_callback_registration_dispatch_and_release_capstone
```

It covers guest registration, a host-held opaque token, typed Wasm callback
invocation, release and stale-token rejection, generation bump on slot reuse,
and native Wasm null/OOB/signature traps. The feature is off by default; rich,
canonical-memory, `f64`, multi-parameter, and async callback lanes remain
fail-closed.

The patch files were produced with:

```sh
git -C /workspace/sonatina-eq-clean format-patch \
  150d327edfa88374802a6cc8089fd77da5fa818b..ac266c210cad7872fc98380a73b4ca363877bc1f
```

Patch `0029` was produced with:

```sh
git -C /workspace/sonatina-host-abi format-patch -1 --start-number 29 \
  -o /workspace/fe-worktrees/mb2/vendor/sonatina/mb2-browser-runtime
```

Patch `0030` was produced with:

```sh
git -C /workspace/sonatina-host-abi format-patch -1 --start-number 30 \
  -o /workspace/fe-worktrees/mb2/vendor/sonatina/mb2-browser-runtime
```

Patch `0031` was produced with:

```sh
git -C /workspace/sonatina-host-abi format-patch -1 --start-number 31 \
  -o /workspace/fe-worktrees/mb2/vendor/sonatina/mb2-browser-runtime
```

Patch `0032` was produced with:

```sh
git -C /workspace/sonatina-host-abi format-patch -1 --start-number 32 \
  -o /workspace/fe-worktrees/mb2/vendor/sonatina/mb2-browser-runtime
```

Patches `0001`–`0028` reconstruct their original hashes when applied with the
recorded author/committer identity and timestamps. Patches `0029`–`0032` are
independently reproducible from dedicated-worktree commits `548b7e54`,
`7ced9661`, `09e2895e`, and `a170ef04`; this checkout does not apply the archive
automatically.
