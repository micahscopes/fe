#!/usr/bin/env bash
# Serve the Fe -> GPU keystone page on http://localhost:8787.
# Runs the generator first if gen/ is missing, so a fresh checkout just works.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ ! -f "$here/gen/layout.json" ]; then
  echo "gen/ missing - generating from the Fe compiler first..."
  ( cd "$here/../.." && cargo run -p fe-codegen --example gen_webgpu_demo )
fi

exec python3 "$here/serve.py"
