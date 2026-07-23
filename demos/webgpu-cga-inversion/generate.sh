#!/usr/bin/env bash
# Generate a browser bundle against its reviewed unpublished Sonatina tree.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
bundle="${CGA_BUNDLE:-default}"
case "$bundle" in
  default)
    expected_sonatina="ed43625bb5680aeab993371e28a8c8e5c7c16f96"
    generator="gen_cga_inversion_demo"
    ;;
  schedule32)
    expected_sonatina="b2601adc8b80b085aae98f9132a035fdfecec5c3"
    generator="gen_cga_schedule32_vec5_demo"
    ;;
  *)
    echo "CGA_BUNDLE must be 'default' or 'schedule32'" >&2
    exit 2
    ;;
esac

if [ -z "${SONATINA_DIR:-}" ]; then
  echo "$bundle generation requires SONATINA_DIR pointing to Sonatina commit $expected_sonatina" >&2
  exit 2
fi
if ! git -C "$SONATINA_DIR" rev-parse --git-dir >/dev/null 2>&1 \
    || [ ! -d "$SONATINA_DIR/crates/codegen" ]; then
  echo "SONATINA_DIR is not the expected Sonatina git checkout: $SONATINA_DIR" >&2
  exit 2
fi
actual_sonatina="$(git -C "$SONATINA_DIR" rev-parse HEAD)"
if [ "$actual_sonatina" != "$expected_sonatina" ]; then
  echo "$bundle requires Sonatina HEAD $expected_sonatina; found $actual_sonatina" >&2
  exit 2
fi
if [ -n "$(git -C "$SONATINA_DIR" status --porcelain)" ]; then
  echo "$bundle requires a clean Sonatina checkout at $expected_sonatina" >&2
  exit 2
fi

fe_revision="$(git -C "$repo" rev-parse HEAD)"
if [ -n "$(git -C "$repo" status --porcelain --untracked-files=no)" ]; then
  echo "$bundle generation requires a Fe checkout with no tracked modifications" >&2
  exit 2
fi
fe_status="$(git -C "$repo" status --porcelain --untracked-files=normal)"
if grep -q '^?? ' <<<"$fe_status"; then
  fe_untracked_present=1
else
  fe_untracked_present=0
fi

lock_backup="$(mktemp "${TMPDIR:-/tmp}/fe-cga-Cargo.lock.XXXXXX")"
cp "$repo/Cargo.lock" "$lock_backup"
restore_lock() {
  cp "$lock_backup" "$repo/Cargo.lock"
  rm -f -- "$lock_backup"
}
trap restore_lock EXIT

( cd "$repo" && \
  FE_CGA_SOURCE_REV="$fe_revision" \
  FE_CGA_SOURCE_UNTRACKED_PRESENT="$fe_untracked_present" \
  cargo \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-ir.path=\"$SONATINA_DIR/crates/ir\"" \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-triple.path=\"$SONATINA_DIR/crates/triple\"" \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-codegen.path=\"$SONATINA_DIR/crates/codegen\"" \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-verifier.path=\"$SONATINA_DIR/crates/verifier\"" \
    run -p fe-codegen --example "$generator" )
