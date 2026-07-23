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

# Browser-profile generators share the exact ac266c21 Sonatina backend. When a
# caller provides any checkout containing Fe's fetchable base, reconstruct the
# reviewed commit internally; cga-d1 deliberately retains its older ed43625b
# pin. The overlay re-enters this script with its clean b260 checkout.
if [ "$demo" != cga-d1 ] && [ -n "${SONATINA_DIR:-}" ] \
    && [ "${FE_SONATINA_OVERLAY_ACTIVE:-0}" != 1 ]; then
  expected_browser_sonatina="ac266c210cad7872fc98380a73b4ca363877bc1f"
  actual_browser_sonatina="$(git -C "$SONATINA_DIR" rev-parse HEAD 2>/dev/null || true)"
  if [ "$actual_browser_sonatina" != "$expected_browser_sonatina" ] \
      || [ -n "$(git -C "$SONATINA_DIR" status --porcelain 2>/dev/null || true)" ]; then
    exec "$here/with-sonatina-overlay.sh" "$0" "${rerun_args[@]}"
  fi
fi

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
  if [ -z "${FE_DEMO_GENERATE_CMD:-}" ] && [ -z "${SONATINA_DIR:-}" ]; then
    "$here/with-sonatina-overlay.sh" "$0" "${rerun_args[@]}"
    return
  fi
  echo "generating $key..."
  if [ -n "${FE_DEMO_GENERATE_CMD:-}" ]; then
    "$FE_DEMO_GENERATE_CMD" "$key"
  else
    "$here/with-browser-cargo.sh" \
      run -p fe-codegen --example "$example"
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
    cga)
      if [ -n "${FE_DEMO_GENERATE_CMD:-}" ]; then
        "$FE_DEMO_GENERATE_CMD" cga
      else
        cga_bundle="$here/webgpu-cga-inversion/gen-schedule32"
        if [ -z "${SONATINA_DIR:-}" ] \
            && [ "${FE_SONATINA_OVERLAY_ACTIVE:-0}" != 1 ] \
            && bundle_needs_generation "${FORCE_DEMO_REGEN:-0}" "$cga_bundle" \
              kernel.fe frag.wgsl layout.json reference.json frag.wasm \
              actor/module.wasm actor/shader.wgsl actor/manifest.json \
              actor/interface.js actor/interface.d.ts \
              actor/runtime/actor-coordinator.js actor/runtime/actor-endpoint.js \
              actor/runtime/actor-router.js actor/runtime/gpu-actor.js \
              actor/runtime/message-port-actor.js \
              actor/runtime/module-worker-actor.js actor-source.fe; then
          "$here/with-sonatina-overlay.sh" "$0" cga
          return
        fi
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
        if [ -z "${SONATINA_DIR:-}" ] \
            && [ "${FE_SONATINA_OVERLAY_ACTIVE:-0}" != 1 ] \
            && bundle_needs_generation "${FORCE_DEMO_REGEN:-0}" "$qcga_bundle" \
              kernel.fe frag.wgsl frag.wasm layout.json reference.json \
              actor-source.fe actor-canonical.wasm actor-interface.js \
              actor-interface.d.ts actor-manifest.json \
              runtime/actor-coordinator.js runtime/actor-endpoint.js \
              runtime/actor-router.js runtime/gpu-actor.js \
              runtime/message-port-actor.js runtime/module-worker-actor.js; then
          "$here/with-sonatina-overlay.sh" "$0" qcga
          return
        fi
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
  python3 "$here/shared/verify_cga_runtime_reuse.py"
else
  generate_one "$demo"
fi

if [ "$serve" = 1 ]; then
  trunk_args=(serve --config "$here/Trunk.toml")
  if [ "$no_watch" = 1 ]; then trunk_args+=(--no-autoreload); fi
  echo "serving browser demos (selected bundle: $demo)..."
  exec trunk "${trunk_args[@]}"
fi
