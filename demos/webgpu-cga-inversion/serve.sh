#!/usr/bin/env bash
# Generate only D1 when needed, then serve the common demos root.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ ! -f "$here/gen/layout.json" ] || [ "${FORCE_CGA_REGEN:-0}" = 1 ]; then
  "$here/generate.sh"
fi
exec python3 "$here/../serve.py"
