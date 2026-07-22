#!/usr/bin/env bash
# Serve the Fe demos ROOT on http://localhost:8788 so BOTH pages work off one
# origin: /webgpu-keystone/ and /webgpu-mandelbrot/. Runs each generator first
# if its gen/ is missing. The CGA demo currently requires an unpublished local
# Sonatina checkout, configurable through SONATINA_DIR.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ ! -f "$here/webgpu-keystone/gen/layout.json" ]; then
  echo "webgpu-keystone/gen missing - generating from the Fe compiler first..."
  ( cd "$here/.." && cargo run -p fe-codegen --example gen_webgpu_demo )
fi
if [ ! -f "$here/webgpu-mandelbrot/gen/layout.json" ]; then
  echo "webgpu-mandelbrot/gen missing - generating from the Fe compiler first..."
  ( cd "$here/.." && cargo run -p fe-codegen --example gen_mandelbrot_demo )
fi
if [ ! -f "$here/webgpu-mandelbrot-interactive/gen/layout.json" ]; then
  echo "webgpu-mandelbrot-interactive/gen missing - generating from the Fe compiler first..."
  ( cd "$here/.." && cargo run -p fe-codegen --example gen_mandelbrot_interactive_demo )
fi
if [ ! -f "$here/webgpu-clifford-interactive/gen/layout.json" ]; then
  echo "webgpu-clifford-interactive/gen missing - generating from the Fe compiler first..."
  ( cd "$here/.." && cargo run -p fe-codegen --example gen_clifford_interactive_demo )
fi
if [ ! -f "$here/webgpu-cga-inversion/gen/layout.json" ] || [ "${FORCE_CGA_REGEN:-0}" = 1 ]; then
  echo "webgpu-cga-inversion/gen missing - generating from the Fe compiler first..."
  sonatina_dir="${SONATINA_DIR:-/workspace/sonatina}"
  if [ ! -d "$sonatina_dir/crates/codegen" ]; then
    echo "CGA generation requires the unpublished Sonatina checkout; set SONATINA_DIR" >&2
    exit 1
  fi
  ( cd "$here/.." && SONATINA_DIR="$sonatina_dir" cargo \
      --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-ir.path=\"$sonatina_dir/crates/ir\"" \
      --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-triple.path=\"$sonatina_dir/crates/triple\"" \
      --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-codegen.path=\"$sonatina_dir/crates/codegen\"" \
      --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-verifier.path=\"$sonatina_dir/crates/verifier\"" \
      run -p fe-codegen --example gen_cga_inversion_demo )
fi

exec python3 "$here/serve.py"
