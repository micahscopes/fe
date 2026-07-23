#!/usr/bin/env bash
# Run Cargo against the reviewed, unpublished browser-backend overlay.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"

if [ "$#" -eq 0 ]; then
  echo "usage: $0 CARGO_ARG [CARGO_ARG ...]" >&2
  exit 2
fi

if [ -z "${FE_DEMO_GENERATION_LOCK_ACTIVE:-}" ]; then
  exec "$here/with-fe-generation-lock.sh" "$0" "$@"
fi
if [ -z "${SONATINA_DIR:-}" ]; then
  exec "$here/with-sonatina-overlay.sh" "$0" "$@"
fi

expected="ac266c210cad7872fc98380a73b4ca363877bc1f"
actual="$(git -C "$SONATINA_DIR" rev-parse HEAD 2>/dev/null || true)"
dirty="$(git -C "$SONATINA_DIR" status --porcelain 2>/dev/null || true)"
if [ "$actual" != "$expected" ] || [ -n "$dirty" ]; then
  echo "browser Cargo requires clean reviewed Sonatina $expected (got ${actual:-nothing})" >&2
  exit 1
fi

demo_tmp_root="${FE_DEMO_TMPDIR:-$repo/output/demo-tmp}"
mkdir -p "$demo_tmp_root"
lock_backup="$(mktemp "$demo_tmp_root/fe-browser-Cargo.lock.XXXXXX")"
cp "$repo/Cargo.lock" "$lock_backup"
restore_lock() {
  cp "$lock_backup" "$repo/Cargo.lock"
  rm -f -- "$lock_backup"
}
trap restore_lock EXIT

target_dir="${CARGO_TARGET_DIR:-$repo/target/fe-browser}"
mkdir -p "$target_dir"
cd "$repo"
echo "using temporary reviewed Sonatina browser overlay $expected" >&2
RUSTC_WRAPPER= CARGO_TARGET_DIR="$target_dir" cargo \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-ir.path=\"$SONATINA_DIR/crates/ir\"" \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-triple.path=\"$SONATINA_DIR/crates/triple\"" \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-codegen.path=\"$SONATINA_DIR/crates/codegen\"" \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-verifier.path=\"$SONATINA_DIR/crates/verifier\"" \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-macros.path=\"$SONATINA_DIR/crates/macros\"" \
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-parser.path=\"$SONATINA_DIR/crates/parser\"" \
  "$@"
