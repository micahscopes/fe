#!/usr/bin/env bash
# Explicit generation and preflight for repository browser demos.
#
# Usage:
#   demos/serve.sh [all|keystone|mandelbrot|mandelbrot-interactive|
#                   clifford-interactive|cga|cga-d1|cga-schedule32|qcga]
# Trunk serving is deliberately separate and has no hidden generation hook.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
demo="${1:-cga}"
if [ "$#" -gt 0 ]; then shift; fi
if [ "$#" -ne 0 ]; then
  echo "usage: demos/serve.sh [DEMO]" >&2
  exit 2
fi

# Browser-profile generators share the exact b2601adc Sonatina backend. When a
# caller provides any checkout containing Fe's fetchable base, reconstruct the
# reviewed commit internally; cga-d1 deliberately retains its older ed43625b
# pin. The overlay re-enters this script with its clean b260 checkout.
if [ "$demo" != cga-d1 ] && [ -n "${SONATINA_DIR:-}" ] \
    && [ "${FE_SONATINA_OVERLAY_ACTIVE:-0}" != 1 ]; then
  expected_browser_sonatina="b2601adc8b80b085aae98f9132a035fdfecec5c3"
  actual_browser_sonatina="$(git -C "$SONATINA_DIR" rev-parse HEAD 2>/dev/null || true)"
  if [ "$actual_browser_sonatina" != "$expected_browser_sonatina" ] \
      || [ -n "$(git -C "$SONATINA_DIR" status --porcelain 2>/dev/null || true)" ]; then
    exec "$here/with-sonatina-overlay.sh" "$0" "$demo"
  fi
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
  elif [ -n "${SONATINA_DIR:-}" ]; then
    (
      lock_backup="$(mktemp "${TMPDIR:-/tmp}/fe-demo-Cargo.lock.XXXXXX")"
      cp "$repo/Cargo.lock" "$lock_backup"
      restore_lock() {
        cp "$lock_backup" "$repo/Cargo.lock"
        rm -f -- "$lock_backup"
      }
      trap restore_lock EXIT
      cd "$repo"
      cargo \
      --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-ir.path=\"$SONATINA_DIR/crates/ir\"" \
      --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-triple.path=\"$SONATINA_DIR/crates/triple\"" \
      --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-codegen.path=\"$SONATINA_DIR/crates/codegen\"" \
      --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-verifier.path=\"$SONATINA_DIR/crates/verifier\"" \
      --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-macros.path=\"$SONATINA_DIR/crates/macros\"" \
      --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-parser.path=\"$SONATINA_DIR/crates/parser\"" \
      run -p fe-codegen --example "$example"
    )
  else
    echo "$key generation requires the reviewed Sonatina browser backend." >&2
    echo "Run with:" >&2
    echo "  SONATINA_DIR=/path/to/sonatina demos/with-sonatina-overlay.sh demos/serve.sh $key" >&2
    exit 2
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
