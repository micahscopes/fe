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
  "$here/webgpu-cga-inversion/generate.sh"
fi

exec python3 "$here/serve.py"
