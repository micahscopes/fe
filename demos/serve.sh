#!/usr/bin/env bash
# One entry point for generating and serving the repository browser demos.
#
# Usage:
#   demos/serve.sh [all|keystone|mandelbrot|mandelbrot-interactive|
#                   clifford-interactive|cga|cga-d1|cga-schedule32|qcga]
#                  [--generate-only]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
demo="${1:-all}"
if [ "$#" -gt 0 ]; then shift; fi
generate_only=0
if [ "${1:-}" = "--generate-only" ]; then
  generate_only=1
  shift
fi
if [ "$#" -ne 0 ]; then
  echo "usage: demos/serve.sh [DEMO] [--generate-only]" >&2
  exit 2
fi

generate_example() {
  local key="$1"
  local example="$2"
  local marker="$3"
  if [ "${FORCE_DEMO_REGEN:-0}" != 1 ] && [ -f "$marker" ]; then
    return
  fi
  echo "generating $key..."
  if [ -n "${FE_DEMO_GENERATE_CMD:-}" ]; then
    "$FE_DEMO_GENERATE_CMD" "$key"
  else
    (cd "$repo" && cargo run -p fe-codegen --example "$example")
  fi
}

generate_one() {
  case "$1" in
    keystone)
      generate_example keystone gen_webgpu_demo \
        "$here/webgpu-keystone/gen/layout.json"
      ;;
    mandelbrot)
      generate_example mandelbrot gen_mandelbrot_demo \
        "$here/webgpu-mandelbrot/gen/layout.json"
      ;;
    mandelbrot-interactive)
      generate_example mandelbrot-interactive gen_mandelbrot_interactive_demo \
        "$here/webgpu-mandelbrot-interactive/gen/layout.json"
      ;;
    clifford-interactive)
      generate_example clifford-interactive gen_clifford_interactive_demo \
        "$here/webgpu-clifford-interactive/gen/layout.json"
      ;;
    cga)
      if [ -n "${FE_DEMO_GENERATE_CMD:-}" ]; then
        "$FE_DEMO_GENERATE_CMD" cga
      else
        FORCE_CGA_REGEN="${FORCE_DEMO_REGEN:-0}" \
          CGA_BUNDLE=schedule32 \
          CGA_BUNDLE_DIR="$here/webgpu-cga-inversion/gen-schedule32" \
          "$here/webgpu-cga-inversion/ensure-assets.sh"
      fi
      ;;
    cga-d1)
      if [ -n "${FE_DEMO_GENERATE_CMD:-}" ]; then
        "$FE_DEMO_GENERATE_CMD" cga-d1
      else
        FORCE_CGA_REGEN="${FORCE_DEMO_REGEN:-0}" \
          CGA_BUNDLE=default \
          CGA_BUNDLE_DIR="$here/webgpu-cga-inversion/gen" \
          "$here/webgpu-cga-inversion/ensure-assets.sh"
      fi
      ;;
    cga-schedule32)
      generate_one cga
      ;;
    qcga)
      if [ -n "${FE_DEMO_GENERATE_CMD:-}" ]; then
        "$FE_DEMO_GENERATE_CMD" qcga
      else
        FORCE_QCGA_REGEN="${FORCE_DEMO_REGEN:-0}" \
          "$here/webgpu-qcga3d-quadric/ensure-assets.sh"
      fi
      ;;
    *)
      echo "unknown demo '$1'" >&2
      echo "choose: all, keystone, mandelbrot, mandelbrot-interactive, clifford-interactive, cga, cga-d1, cga-schedule32, qcga" >&2
      exit 2
      ;;
  esac
}

if [ "$demo" = all ]; then
  for selected in keystone mandelbrot mandelbrot-interactive \
    clifford-interactive cga qcga
  do
    generate_one "$selected"
  done
else
  generate_one "$demo"
fi

if [ "$generate_only" = 1 ]; then
  exit 0
fi

echo "serving demos at http://${HOST:-127.0.0.1}:${PORT:-8788}/"
exec python3 "$here/serve.py"
