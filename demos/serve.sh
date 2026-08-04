#!/usr/bin/env bash
# Explicit generation and preflight for repository browser demos.
#
# Usage:
#   demos/serve.sh [DEMO] [--serve] [--no-watch]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
demo=cga
serve=0
no_watch=0
if [ "$#" -gt 0 ] && [[ "$1" != --* ]]; then
  demo="$1"
  shift
fi
while [ "$#" -gt 0 ]; do
  case "$1" in
    --serve) serve=1 ;;
    --no-watch) no_watch=1 ;;
    -h|--help)
      echo "usage: demos/serve.sh [DEMO] [--serve] [--no-watch]"
      exit 0
      ;;
    *)
      echo "usage: demos/serve.sh [DEMO] [--serve] [--no-watch]" >&2
      exit 2
      ;;
  esac
  shift
done
if [ "$no_watch" = 1 ] && [ "$serve" != 1 ]; then
  echo "--no-watch requires --serve" >&2
  exit 2
fi
rerun_args=("$demo")
if [ "$serve" = 1 ]; then rerun_args+=(--serve); fi
if [ "$no_watch" = 1 ]; then rerun_args+=(--no-watch); fi
if [ "${FE_DEMO_GENERATION_LOCK_ACTIVE:-0}" != 1 ]; then
  exec "$here/with-fe-generation-lock.sh" "$0" "${rerun_args[@]}"
fi

# The workspace pins the reviewed browser backend ac266c21 directly, so
# generation is plain locked cargo. The former overlay reconstructed that commit
# from tracked patches because the manifest still said 150d327e; that is gone.

generate_example() {
  local key="$1"
  local example="$2"
  shift 2
  if [ "${FORCE_DEMO_REGEN:-0}" != 1 ]; then
    local complete=1
    local marker
    for marker in "$@"; do
      if [ ! -f "$marker" ]; then
        complete=0
        break
      fi
    done
    if [ "$complete" = 1 ]; then
      return
    fi
  fi
  echo "generating $key..."
  if [ -n "${FE_DEMO_GENERATE_CMD:-}" ]; then
    "$FE_DEMO_GENERATE_CMD" "$key"
  else
    (cd "$repo" && cargo run --locked -p fe-codegen --example "$example")
  fi
}

generate_via_script() {
  local key="$1"
  local script="$2"
  shift 2
  if [ "${FORCE_DEMO_REGEN:-0}" != 1 ]; then
    local complete=1
    local marker
    for marker in "$@"; do
      if [ ! -f "$marker" ]; then
        complete=0
        break
      fi
    done
    if [ "$complete" = 1 ]; then
      return
    fi
  fi
  echo "generating $key..."
  if [ -n "${FE_DEMO_GENERATE_CMD:-}" ]; then
    "$FE_DEMO_GENERATE_CMD" "$key"
  else
    "$script"
  fi
}

bundle_needs_generation() {
  local force="$1"
  local directory="$2"
  shift 2
  if [ "$force" = 1 ]; then
    return 0
  fi
  local asset
  for asset in "$@"; do
    if [ ! -f "$directory/$asset" ]; then
      return 0
    fi
  done
  return 1
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
        "$here/webgpu-mandelbrot-interactive/gen/layout.json" \
        "$here/webgpu-mandelbrot-interactive/gen/actor/manifest.json" \
        "$here/webgpu-mandelbrot-interactive/gen/actor/runtime/module-worker-actor.js"
      ;;
    clifford-interactive)
      generate_example clifford-interactive gen_clifford_interactive_demo \
        "$here/webgpu-clifford-interactive/gen/layout.json"
      ;;
    cga3d-interactive)
      generate_via_script cga3d-interactive "$here/webgpu-cga3d-interactive/generate.sh" \
        "$here/webgpu-cga3d-interactive/gen/manifest.json" \
        "$here/webgpu-cga3d-interactive/gen/ctl.json"
      ;;
    qcga-interactive)
      generate_via_script qcga-interactive "$here/webgpu-qcga-interactive/generate.sh" \
        "$here/webgpu-qcga-interactive/gen/manifest.json" \
        "$here/webgpu-qcga-interactive/gen/ctl.json"
      ;;
    desargues-interactive)
      generate_via_script desargues-interactive "$here/webgpu-desargues-interactive/generate.sh" \
        "$here/webgpu-desargues-interactive/gen/manifest.json" \
        "$here/webgpu-desargues-interactive/gen/ctl.json"
      ;;
    cga)
      if [ -n "${FE_DEMO_GENERATE_CMD:-}" ]; then
        "$FE_DEMO_GENERATE_CMD" cga
      else
        cga_bundle="$here/webgpu-cga-inversion/gen-schedule32"
        FORCE_CGA_REGEN="${FORCE_DEMO_REGEN:-0}" \
          CGA_BUNDLE=schedule32 \
          CGA_BUNDLE_DIR="$cga_bundle" \
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
        qcga_bundle="$here/webgpu-qcga3d-quadric/gen"
        FORCE_QCGA_REGEN="${FORCE_DEMO_REGEN:-0}" \
          "$here/webgpu-qcga3d-quadric/ensure-assets.sh"
      fi
      ;;
    *)
      echo "unknown demo '$1'" >&2
      echo "choose: all, keystone, mandelbrot, mandelbrot-interactive, clifford-interactive, cga, cga-d1, cga-schedule32, qcga, cga3d-interactive, qcga-interactive, desargues-interactive" >&2
      exit 2
      ;;
  esac
}

if [ "$demo" = all ]; then
  for selected in keystone mandelbrot mandelbrot-interactive \
    clifford-interactive cga qcga \
    cga3d-interactive qcga-interactive desargues-interactive
  do
    generate_one "$selected"
  done
  python3 "$here/shared/verify_cga_runtime_reuse.py"
else
  generate_one "$demo"
fi

if [ "$serve" = 1 ]; then
  trunk_args=(serve --config "$here/Trunk.toml")
  if [ "$no_watch" = 1 ]; then trunk_args+=(--no-autoreload); fi
  # Trunk's clap parser expects an explicit boolean, while the conventional
  # NO_COLOR environment variable is commonly exported as `1`.
  if [ "${NO_COLOR:-}" = 1 ]; then export NO_COLOR=true; fi
  echo "serving browser demos (selected bundle: $demo)..."
  exec trunk "${trunk_args[@]}"
fi
