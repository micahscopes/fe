#!/usr/bin/env sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
workspace_dir=$(CDPATH= cd -- "$script_dir/../.." && pwd)
package_dir="$script_dir/gen/compiler"

wasm-pack build \
  "$workspace_dir/crates/browser-compiler" \
  --target web \
  --release \
  --out-dir "$package_dir"

bun "$script_dir/verify-browser-compiler.mjs" "$package_dir"

