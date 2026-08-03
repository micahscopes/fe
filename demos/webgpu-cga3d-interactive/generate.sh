#!/usr/bin/env bash
# Regenerate demos/webgpu-cga3d-interactive/gen/ (gitignored; never hand-edited).
#
# Two independent halves, each produced by REAL Fe tooling, never hand-rolled:
#   - the render fragment (the cga3d actor's `shade`, lambda/theta/zoom
#     uniforms) via the shipped `fe web build` CLI against
#     demos/sketches/cga3d -- the same command any cga3d bundle uses;
#   - the drag controls (`update_pencil`) via the `gen_cga3d_interactive_ctl`
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
render_tmp="$(mktemp -d "${TMPDIR:-/tmp}/cga3d-interactive-render.XXXXXX")"
trap 'rm -rf "$render_tmp"' EXIT
rm -rf "$render_tmp"
"$fe_bin" web build --out "$render_tmp" "$repo/demos/sketches/cga3d"
cp "$render_tmp/module.wasm" "$gen_dir/frag.wasm"
cp "$render_tmp/shader.wgsl" "$gen_dir/frag.wgsl"
cp "$render_tmp/manifest.json" "$gen_dir/manifest.json"
cp "$repo/demos/sketches/cga3d/src/lib.fe" "$gen_dir/kernel.fe"
echo "render fragment: frag.wasm, frag.wgsl, manifest.json, kernel.fe (via fe web build)"

# --- 2. The drag controls, via the gated Fe-codegen example. ---------------
( cd "$repo" && cargo run -p fe-codegen --example gen_cga3d_interactive_ctl )

echo "wrote $gen_dir:"
ls -la "$gen_dir"
