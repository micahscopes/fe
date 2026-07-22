#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
required=(kernel.fe frag.wgsl frag.wasm layout.json reference.json)
missing=()
for asset in "${required[@]}"; do [ -f "$here/gen/$asset" ] || missing+=("$asset"); done
if [ "${FORCE_QCGA_REGEN:-0}" = 1 ] || [ "${#missing[@]}" -ne 0 ]; then
  if [ "${#missing[@]}" -ne 0 ]; then printf 'QCGA bundle missing:' >&2; printf ' %s' "${missing[@]}" >&2; printf '\n' >&2; fi
  "$here/generate.sh"
fi
python3 "$here/verify-assets.py"
