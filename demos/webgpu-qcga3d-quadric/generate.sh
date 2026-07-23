#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
if [ "${FE_DEMO_GENERATION_LOCK_ACTIVE:-0}" != 1 ]; then
  exec "$repo/demos/with-fe-generation-lock.sh" "$0" "$@"
fi
expected_sonatina="ac266c210cad7872fc98380a73b4ca363877bc1f"

if [ -z "${SONATINA_DIR:-}" ]; then
  echo "QCGA generation requires SONATINA_DIR at Sonatina $expected_sonatina" >&2
  exit 2
fi
actual="$(git -C "$SONATINA_DIR" rev-parse HEAD 2>/dev/null || true)"
if [ "$actual" != "$expected_sonatina" ]; then
  echo "QCGA requires Sonatina HEAD $expected_sonatina; found ${actual:-not-a-checkout}" >&2
  exit 2
fi
if [ -n "$(git -C "$SONATINA_DIR" status --porcelain)" ]; then
  echo "QCGA generation requires a clean Sonatina checkout" >&2
  exit 2
fi
if [ -n "$(git -C "$repo" status --porcelain --untracked-files=no)" ]; then
  echo "QCGA generation requires a Fe checkout with no tracked modifications" >&2
  exit 2
fi

"$repo/demos/with-browser-cargo.sh" \
  run -p fe-codegen --example gen_qcga3d_quadric_demo
