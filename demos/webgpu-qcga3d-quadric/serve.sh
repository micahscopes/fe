#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$here/ensure-assets.sh"
exec python3 "$here/../serve.py"
