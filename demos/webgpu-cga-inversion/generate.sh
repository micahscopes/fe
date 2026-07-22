#!/usr/bin/env bash
# Generate the D1 browser bundle against the reviewed unpublished Sonatina tree.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
expected_sonatina="ed43625bb5680aeab993371e28a8c8e5c7c16f96"

if [ -z "${SONATINA_DIR:-}" ]; then
  echo "D1 generation requires SONATINA_DIR pointing to Sonatina commit $expected_sonatina" >&2
  exit 2
fi
if ! git -C "$SONATINA_DIR" rev-parse --git-dir >/dev/null 2>&1 \
    || [ ! -d "$SONATINA_DIR/crates/codegen" ]; then
  echo "SONATINA_DIR is not the expected Sonatina git checkout: $SONATINA_DIR" >&2
  exit 2
fi
actual_sonatina="$(git -C "$SONATINA_DIR" rev-parse HEAD)"
if [ "$actual_sonatina" != "$expected_sonatina" ]; then
  echo "D1 requires Sonatina HEAD $expected_sonatina; found $actual_sonatina" >&2
  exit 2
fi
if [ -n "$(git -C "$SONATINA_DIR" status --porcelain)" ]; then
  echo "D1 requires a clean Sonatina checkout at $expected_sonatina" >&2
  exit 2
fi

lock_backup="$(mktemp "${TMPDIR:-/tmp}/fe-cga-Cargo.lock.XXXXXX")"
cp "$repo/Cargo.lock" "$lock_backup"
restore_lock() {
  cp "$lock_backup" "$repo/Cargo.lock"
  rm -f -- "$lock_backup"
}
trap restore_lock EXIT

( cd "$repo" && cargo \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-ir.path=\"$SONATINA_DIR/crates/ir\"" \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-triple.path=\"$SONATINA_DIR/crates/triple\"" \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-codegen.path=\"$SONATINA_DIR/crates/codegen\"" \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-verifier.path=\"$SONATINA_DIR/crates/verifier\"" \
    run -p fe-codegen --example gen_cga_inversion_demo )
