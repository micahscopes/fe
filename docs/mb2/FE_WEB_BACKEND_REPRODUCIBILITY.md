# `fe web` backend reproducibility audit

Audited 2026-07-23 against Fe commit `eece0c08d` and reviewed Sonatina commit
`ac266c210cad7872fc98380a73b4ca363877bc1f`.

## Current facts

The root workspace declares four Sonatina Git dependencies at
`150d327edfa88374802a6cc8089fd77da5fa818b`. The lockfile resolves six
Sonatina packages from that same source: `ir`, `triple`, `codegen`,
`verifier`, `macros`, and `parser`.

That base commit is fetchable from
`micahscopes/sonatina:refs/heads/mb2-render-mode`. The reviewed browser backend
commit `ac266c21` was absent from the advertised refs of both
`micahscopes/sonatina` and `fe-lang/sonatina` at audit time. A clean direct
command therefore cannot name the backend Fe currently needs. In fact:

```text
$ cargo check -p fe-codegen --locked --offline
error: cannot update the lock file ... because --locked was passed
```

The repository does retain the reviewed delta under
`vendor/sonatina/mb2-browser-runtime/`:

- 28 ordered mail patches plus `SHA256SUMS`;
- approximately 528 KiB of patch payload;
- 10,812 patch lines;
- an exact reconstruction check for `ac266c21`;
- a 59-file upstream delta with 6,545 insertions and 830 deletions.

`demos/with-sonatina-overlay.sh` fetches only the pinned base, verifies the
patch checksums, applies the series in an isolated checkout, and rejects any
result other than the exact target commit. This is reproducible generation
infrastructure, but Cargo cannot run it while resolving a normal workspace
dependency. Consequently `demos/fe-web` remains a compatibility launcher
rather than the intended plain `fe web` command.

## Supported local alternatives

### Vendor/path dependencies

Cargo supports replacing the Git dependencies with checked-in path
dependencies. This is the only direct, clean-checkout solution that requires
no reachable target Git commit and no pre-Cargo launcher.

The reviewed Sonatina working tree is approximately 9.14 MiB of raw files
(about 12 MiB on disk without `.git`). The six directly or transitively needed
crate directories account for nearly all of that because `sonatina-codegen`
itself is about 9.3 MiB on disk. A careful package-style vendor could omit
upstream-only test fixtures, but it would still duplicate several MiB of
compiler implementation, workspace metadata, license/readme material, and six
interdependent manifests.

This is technically sound, but it is not a small fix. It creates a second
source-of-truth representation alongside the substantially smaller reviewed
patch series. Every upstream rebase would require refreshing a large vendored
tree and proving that it still represents the reviewed commit. Do this only if
offline-first builds are a product requirement or the backend cannot be
published durably.

### Cargo source replacement or `[patch]`

Neither mechanism applies a mail-patch series. Both ultimately require either
a reachable registry/Git source or complete local crate source directories.
Pointing `.cargo/config.toml` at a generated directory does not help a clean
checkout: that directory must exist before Cargo begins dependency resolution.

A checked-in Git bundle or compressed source archive has the same bootstrap
problem. It can reduce repository bytes, but a launcher must unpack or clone it
before Cargo runs, reproducing the current wrapper boundary under a different
name.

### Current reviewed overlay

The current overlay is the smallest repository-local representation and has
strong provenance. It is appropriate for demos and CI commands that explicitly
enter the wrapper. It cannot honestly be described as plain `cargo` or plain
`fe web`, and should not be hidden behind an implicit shell hook.

## Recommended completion path

Publish the reviewed commit, or an upstream replacement containing the same
required APIs, to a durable Sonatina remote:

1. Push the reviewed 28-commit series to a named review branch and ensure
   `ac266c210cad7872fc98380a73b4ca363877bc1f` is fetchable by a fresh clone.
2. Review/merge the series upstream where practical. If its final commit
   changes during review, record the replacement commit and the relationship
   to this patch series.
3. Repin the four root workspace dependencies to the durable commit and
   regenerate `Cargo.lock`, ensuring all six Sonatina packages resolve from the
   same source revision.
4. Prove a fresh checkout with no `SONATINA_DIR`, Cargo patches, or warm Git
   cache:

   ```sh
   cargo check --workspace --locked
   cargo test -p fe --bin fe web_serve
   cargo test -p fe-codegen --test fco_cga80_direct_lanes \
     --test fco_cga80_direct_de_spirv --test wasm_e2e --test spirv_e2e
   cargo run -p fe -- web build path/to/kernel.fe \
     --entry shade --mode render --out /new/output/path
   ```

5. Only after those gates pass, remove `demos/fe-web` and the six-package Cargo
   patching layer. Retain the application-specific flagship generators for
   oracle, plan-witness, provenance, and legacy artifact-contract work that the
   generic `WebBundle` does not perform.

No repository-local edit can make an unreachable Git revision directly
resolvable by Cargo without either adding the full source tree or retaining a
bootstrap wrapper. Given the measured 528 KiB reviewed patch representation
versus the multi-megabyte vendor duplication, durable publication and a normal
lockfile repin is the preferred path.
