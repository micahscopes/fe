#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
expected_sonatina="b2601adc8b80b085aae98f9132a035fdfecec5c3"

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

lock_backup="$(mktemp "${TMPDIR:-/tmp}/fe-qcga-Cargo.lock.XXXXXX")"
cp "$repo/Cargo.lock" "$lock_backup"
restore_lock() { cp "$lock_backup" "$repo/Cargo.lock"; rm -f -- "$lock_backup"; }
trap restore_lock EXIT

( cd "$repo" && cargo \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-ir.path=\"$SONATINA_DIR/crates/ir\"" \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-triple.path=\"$SONATINA_DIR/crates/triple\"" \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-codegen.path=\"$SONATINA_DIR/crates/codegen\"" \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-verifier.path=\"$SONATINA_DIR/crates/verifier\"" \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-macros.path=\"$SONATINA_DIR/crates/macros\"" \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-parser.path=\"$SONATINA_DIR/crates/parser\"" \
  run -p fe-codegen --example gen_qcga3d_quadric_demo )
