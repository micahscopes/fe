# Reviewed Sonatina browser-backend overlay

These 25 `git format-patch` files reproduce the exact unpublished Sonatina
commit `b2601adc8b80b085aae98f9132a035fdfecec5c3` from the remotely fetchable Fe
workspace base `150d327edfa88374802a6cc8089fd77da5fa818b`.

The series is committed evidence for the f32 IR/Wasm/SPIR-V substrate, structured
shader control flow, typed shader inputs, scalar comparisons and shifts, and the
opt-in canonical Wasm arena used by the browser demos. In particular:

- `0001`–`0021` provide the browser-profile f32 and SPIR-V path;
- `0022` adds `WasmBackend::with_canonical_arena`;
- `0023`–`0025` add typed scalar memory, dynamic arena allocation, and bitwise
  lowering needed by canonical actor Wasm.

`demos/with-sonatina-overlay.sh` applies the series to an isolated temporary
clone, verifies that the resulting commit is exactly `b2601adc`, runs the given
command with that checkout as `SONATINA_DIR`, and removes the clone. With no
`SONATINA_DIR`, it fetches the exact base from the pinned Sonatina remote branch
once and retains it under `target/fe-browser-cache`; `FE_BROWSER_CACHE_DIR`
relocates the cache and `FE_BROWSER_OFFLINE=1` forbids fetching on a miss. A
caller-supplied checkout remains a supported offline/source override and is
never patched. Cache mutation is serialized, `SHA256SUMS` is checked before
applying anything, and warm-cache reconstruction performs no network operation.
`demos/serve.sh` invokes the overlay internally, so this is temporary build
plumbing—not a second user-facing build command. It can be removed once the
reviewed backend (or its upstream replacement) is remotely fetchable.

The patch files were produced with:

```sh
git -C /workspace/sonatina-eq-clean format-patch \
  150d327edfa88374802a6cc8089fd77da5fa818b..b2601adc8b80b085aae98f9132a035fdfecec5c3
```

They reconstruct the original hashes because those commits use identical author
and committer identities/timestamps; the overlay applies them with
`--committer-date-is-author-date` and the recorded `trial <trial@local>`
identity.
