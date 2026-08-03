#!/usr/bin/env bash
# Regenerate demos/webgpu-desargues-interactive/gen/ (gitignored; never hand-edited).
#
# Two independent halves, each produced by REAL Fe tooling, never hand-rolled:
#   - the render fragment (the desargues actor's `shade`, sweep/spin/zoom
#     uniforms) via the shipped `fe web build` CLI against
#     demos/sketches/desargues -- the same command any bundle uses; the
#     per-pixel geometry is the CTFE-planned meet/join (gaplay), dual-gated in
#     crates/codegen/tests/pga2d_meet_join_plan.rs;
#   - the drag controls (`update_desargues`) via the gen_desargues_interactive_ctl
#     Fe-codegen example, which hard-gates the wasm export signature and an
#     independent Rust oracle (bit-exact f32) before writing anything.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
gen_dir="$here/gen"

fe_bin="$repo/target/release/fe"
if [ ! -x "$fe_bin" ]; then
  echo "building release fe CLI (one-time)..." >&2
  ( cd "$repo" && cargo build --release -p fe )
fi

mkdir -p "$gen_dir"

# --- 1. The render fragment, via the real `fe web build` CLI. --------------
render_tmp="$(mktemp -d "${TMPDIR:-/tmp}/desargues-interactive-render.XXXXXX")"
trap 'rm -rf "$render_tmp"' EXIT
rm -rf "$render_tmp"
"$fe_bin" web build --out "$render_tmp" "$repo/demos/sketches/desargues"
cp "$render_tmp/module.wasm" "$gen_dir/frag.wasm"
cp "$render_tmp/shader.wgsl" "$gen_dir/frag.wgsl"
cp "$render_tmp/manifest.json" "$gen_dir/manifest.json"
cp "$repo/demos/sketches/desargues/src/lib.fe" "$gen_dir/kernel.fe"
echo "render fragment: frag.wasm, frag.wgsl, manifest.json, kernel.fe (via fe web build)"

# --- 2. The drag controls, via the gated Fe-codegen example. ---------------
( cd "$repo" && cargo run -p fe-codegen --example gen_desargues_interactive_ctl )

echo "wrote $gen_dir:"
ls -la "$gen_dir"
