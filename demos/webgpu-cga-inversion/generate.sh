#!/usr/bin/env bash
# Generate a browser bundle against its reviewed unpublished Sonatina tree.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
if [ "${FE_DEMO_GENERATION_LOCK_ACTIVE:-0}" != 1 ]; then
  exec "$repo/demos/with-fe-generation-lock.sh" "$0" "$@"
fi
bundle="${CGA_BUNDLE:-default}"
case "$bundle" in
  default)
    expected_sonatina="ed43625bb5680aeab993371e28a8c8e5c7c16f96"
    generator="gen_cga_inversion_demo"
    ;;
  schedule32)
    expected_sonatina="ac266c210cad7872fc98380a73b4ca363877bc1f"
    generator="gen_cga_schedule32_vec5_demo"
    ;;
  *)
    echo "CGA_BUNDLE must be 'default' or 'schedule32'" >&2
    exit 2
    ;;
esac

# The workspace manifest pins the reviewed browser backend ac266c21 directly, so
# Cargo enforces the revision. The former SONATINA_DIR precondition was a
# hand-maintained restatement of that invariant, needed only while the manifest
# said 150d327e and the overlay patched it at build time.
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

if [ "$bundle" = schedule32 ]; then
  FE_CGA_SOURCE_REV="$fe_revision" \
  FE_CGA_SOURCE_UNTRACKED_PRESENT="$fe_untracked_present" \
    cargo run --locked --manifest-path "$repo/Cargo.toml" \
    -p fe-codegen --example "$generator"
else
  # Legacy D1 is pinned to an older backend and cannot use the browser runner's
  # exact ac266c21 contract.
  demo_tmp_root="${FE_DEMO_TMPDIR:-$repo/output/demo-tmp}"
  mkdir -p "$demo_tmp_root"
  lock_backup="$(mktemp "$demo_tmp_root/fe-cga-Cargo.lock.XXXXXX")"
  cp "$repo/Cargo.lock" "$lock_backup"
  restore_lock() {
    cp "$lock_backup" "$repo/Cargo.lock"
    rm -f -- "$lock_backup"
  }
  trap restore_lock EXIT
  ( cd "$repo" && \
    FE_CGA_SOURCE_REV="$fe_revision" \
    FE_CGA_SOURCE_UNTRACKED_PRESENT="$fe_untracked_present" \
    RUSTC_WRAPPER="${FE_DEMO_RUSTC_WRAPPER:-}" cargo \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-ir.path=\"$SONATINA_DIR/crates/ir\"" \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-triple.path=\"$SONATINA_DIR/crates/triple\"" \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-codegen.path=\"$SONATINA_DIR/crates/codegen\"" \
    --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-verifier.path=\"$SONATINA_DIR/crates/verifier\"" \
    run -p fe-codegen --example "$generator" )
fi
