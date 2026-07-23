#!/usr/bin/env bash
# Ensure the typed CGA bundle is complete and schema-valid before serve/smoke.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bundle="${CGA_BUNDLE_DIR:-$here/gen}"
required=(kernel.fe frag.wgsl layout.json reference.json frag.wasm)
if [ "${CGA_BUNDLE:-}" = schedule32 ] || [ "$(basename "$bundle")" = gen-schedule32 ]; then
  required+=(actor/module.wasm actor/shader.wgsl actor/manifest.json
    actor/interface.js actor/interface.d.ts actor/runtime/actor-coordinator.js
    actor/runtime/actor-endpoint.js actor/runtime/actor-router.js
    actor/runtime/gpu-actor.js actor/runtime/message-port-actor.js
    actor/runtime/module-worker-actor.js actor-source.fe)
fi
missing=()
for asset in "${required[@]}"; do
  if [ ! -f "$bundle/$asset" ]; then missing+=("$asset"); fi
done

if [ "${FORCE_CGA_REGEN:-0}" = 1 ] || [ "${#missing[@]}" -ne 0 ]; then
  if [ "${#missing[@]}" -ne 0 ]; then
    printf 'CGA bundle incomplete in %s; missing:' "$bundle" >&2
    printf ' %s' "${missing[@]}" >&2
    printf '\n' >&2
  fi
  if [ -n "${CGA_GENERATE_CMD:-}" ]; then
    "$CGA_GENERATE_CMD"
  else
    if [ -z "${SONATINA_DIR:-}" ]; then
      echo "Set SONATINA_DIR to the pinned local Sonatina checkout, then rerun." >&2
      exit 2
    fi
    "$here/generate.sh"
  fi
fi

if [ -n "${CGA_VERIFY_CMD:-}" ]; then
  "$CGA_VERIFY_CMD"
else
  python3 "$here/verify-assets.py"
fi
