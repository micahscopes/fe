# Classic Quilting portable Fe integration evidence

Date: 2026-09-02

This gate supersedes the mutable-worktree caveat in the M0/M1 evidence notes.
The checked fixture ABI, Fe math oracles, and fixed raster now resolve from one
ordinary Fe checkout; none of their manifests name a sibling worktree.

## Exact inputs

- Fe/MB2 integration base: `08a8750e6`
- Sonatina source: `83d2d3b1fcb28e61edf083b0e6f671500cca5d70`
- public Quilting oracle source:
  `902ce0642faa7c9bdfe3b115ba55bdfbc529eda6`
- smallest fixture SHA-256:
  `8efbe07edc92197c5e94e35954dc5ed43927dd12f32d3cb66dce62e082a4294a`
- generated Fe topology SHA-256:
  `3928528bac1ad0cf29351e8fde1f3e6ef650d7bc015e8c5eff8ab62b507dbe0a`

The standalone Rust oracle owns its lockfile through an empty `[workspace]`
table. Its Fe dependencies are repository-relative. Its Quilting dependency is
the exact public Git revision above, rather than an adjacent filesystem path.
The atlas, constrained-Delaunay, sampling, and triangle sources used by the
fixture exporter are unchanged between that public revision and the current
Quilting audit revision `a39ec9f`. Later patch/quaternion additions do not alter
the M1 operations; the release oracles below independently prove their f32
results.

## Integration correction

The broad fixed-raster check exposed that `Derive` accepts a unary constraint
constructor, while the original `GpuLayout<StorageLayout>` provider attempted
to pass a partially applied two-argument trait constructor. Storage derivation
now emits `StorageGpuLayout` evidence and a blanket implementation projects it
as the public `GpuLayout<StorageLayout>` view. This is a compile-time relation:
there is no runtime wrapper, field table, host reducer, or generated shim.

The storage-layout oracle proves exact 4-byte scalar alignment and offsets
`0/4/8` for an `f32/u32/f32` record. A separate rejection fixture proves a
non-host-shareable `bool` field still fails closed. The mixed-storage actor then
proves the derived layout, manifest, and WGSL agree.

## Reproduced release gates

All commands ran in the isolated integration checkout. Expensive compiler and
GPU gates used release artifacts.

```text
cargo test --locked --release --manifest-path tools/classic-quilting-artifact-abi/Cargo.toml
# 7 passed

cargo test --locked --release --manifest-path tools/classic-quilting-artifact-abi/Cargo.toml --features quilting-export
# 11 passed; committed fixture bytes match the pinned Quilting exporter

cargo run --locked --release --manifest-path tools/classic-quilting-artifact-abi/Cargo.toml --bin generate-classic-quilting-fixed-raster -- fixtures/classic-quilting/v1/direct-seed42-k1-1-1.cqa tools/classic-quilting-artifact-abi/target/regenerated-fixed-topology.fe
cmp ingots/classic_quilting_fixed_raster/src/fixed_topology.fe tools/classic-quilting-artifact-abi/target/regenerated-fixed-topology.fe
# byte-identical; SHA-256 3928528b...be0a

fe fmt --check ingots/quilting_domain
fe fmt --check ingots/quilting_quaternion
fe fmt --check ingots/quilting_qb
fe fmt --check ingots/classic_quilting_oracle
fe fmt --check ingots/classic_quilting_fixed_raster
fe check --profile release ingots/classic_quilting_fixed_raster
# all passed with the compiler built from this integration checkout

cargo test --release -p fe-codegen --test gpu_layout_oracle
# 2 passed

cargo test --release -p fe-codegen --test actor_construct mixed_scalar_storage_layout_reconciles_fco_manifest_and_wgsl
# 1 passed

cargo test --locked --release --manifest-path tools/classic-quilting-artifact-abi/Cargo.toml --features fe-oracle -- --nocapture
# 14 passed

LD_LIBRARY_PATH=/nix/store/7krvb015vp4wq7lj6v3wadjy4q9asc8q-vulkan-loader-1.4.341.0/lib:/run/opengl-driver/lib \
VK_ICD_FILENAMES=/run/opengl-driver/share/vulkan/icd.d/lvp_icd.x86_64.json \
WGPU_BACKEND=vulkan \
cargo test --locked --release --manifest-path tools/classic-quilting-artifact-abi/Cargo.toml --features raster-oracle -- --nocapture
# 17 passed on llvmpipe (LLVM 21.1.8, 256 bits)

cargo clippy --locked --release --manifest-path tools/classic-quilting-artifact-abi/Cargo.toml --all-targets --all-features --no-deps -- -D warnings
# passed
```

The remaining M1 work is presentation, not oracle plumbing: blue-noise and
constrained-Delaunay teaching views, boundary/interior classification, and a
selected-triangle diagnostic over this same frozen fixture. M2 then replaces
fixed topology with typed indexed atlas resources and reactive adaptive LoD.
