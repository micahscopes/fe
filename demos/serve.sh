#!/usr/bin/env bash
# Serve the Fe demos ROOT on http://localhost:8788 so BOTH pages work off one
# origin: /webgpu-keystone/ and /webgpu-mandelbrot/. Runs each generator first
# if its gen/ is missing, so a fresh checkout just works.
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

exec python3 "$here/serve.py"
