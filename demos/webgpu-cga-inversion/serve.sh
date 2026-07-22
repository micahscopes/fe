#!/usr/bin/env bash
# Generate only D1 when needed, then serve the common demos root.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$here/ensure-assets.sh"
exec python3 "$here/../serve.py"
