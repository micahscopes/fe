#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
if [ "${FE_DEMO_GENERATION_LOCK_ACTIVE:-0}" != 1 ]; then
  exec "$repo/demos/with-fe-generation-lock.sh" "$0" "$@"
fi
# The workspace manifest pins the reviewed browser backend ac266c21 directly, so
# Cargo enforces the revision. The former SONATINA_DIR precondition was a
# hand-maintained restatement of that invariant, needed only while the manifest
# said 150d327e and the overlay patched it at build time.
if [ -n "$(git -C "$repo" status --porcelain --untracked-files=no)" ]; then
  echo "QCGA generation requires a Fe checkout with no tracked modifications" >&2
  exit 2
fi

cargo run --locked --manifest-path "$repo/Cargo.toml" \
  -p fe-codegen --example gen_qcga3d_quadric_demo
