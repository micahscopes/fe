#!/usr/bin/env bash
# Reconstruct the reviewed browser-backend Sonatina commit from tracked patches,
# without modifying the caller's checkout.
set -euo pipefail

base="150d327edfa88374802a6cc8089fd77da5fa818b"
target="b2601adc8b80b085aae98f9132a035fdfecec5c3"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
patches="$repo/vendor/sonatina/mb2-browser-runtime"

if [ "$#" -eq 0 ]; then
  echo "usage: SONATINA_DIR=/path/to/sonatina $0 COMMAND [ARG ...]" >&2
  exit 2
fi
if [ -z "${SONATINA_DIR:-}" ] || ! git -C "$SONATINA_DIR" rev-parse --git-dir >/dev/null 2>&1; then
  echo "SONATINA_DIR must identify a Sonatina git checkout containing $base" >&2
  exit 2
fi
if ! git -C "$SONATINA_DIR" cat-file -e "$base^{commit}" 2>/dev/null; then
  echo "SONATINA_DIR does not contain browser-backend base $base" >&2
  exit 2
fi
if ! (cd "$patches" && sha256sum --check --quiet SHA256SUMS); then
  echo "tracked Sonatina browser-backend patch checksum mismatch" >&2
  exit 1
fi

actual="$(git -C "$SONATINA_DIR" rev-parse HEAD)"
if [ "$actual" = "$target" ] && [ -z "$(git -C "$SONATINA_DIR" status --porcelain)" ]; then
  exec "$@"
fi

overlay_root="$(mktemp -d "${TMPDIR:-/tmp}/fe-sonatina-overlay.XXXXXX")"
cleanup() {
  case "$overlay_root" in
    "${TMPDIR:-/tmp}"/fe-sonatina-overlay.*) rm -rf -- "$overlay_root" ;;
    *) echo "refusing to remove unexpected overlay path: $overlay_root" >&2 ;;
  esac
}
trap cleanup EXIT

overlay="$overlay_root/sonatina"
git clone --quiet --shared --no-checkout "$SONATINA_DIR" "$overlay"
git -C "$overlay" checkout --quiet --detach "$base"
git -C "$overlay" \
  -c user.name=trial -c user.email=trial@local \
  am --quiet --committer-date-is-author-date "$patches"/*.patch
actual="$(git -C "$overlay" rev-parse HEAD)"
if [ "$actual" != "$target" ] || [ -n "$(git -C "$overlay" status --porcelain)" ]; then
  echo "tracked Sonatina patch series did not reconstruct $target (got $actual)" >&2
  exit 1
fi

export SONATINA_DIR="$overlay"
export FE_SONATINA_OVERLAY_ACTIVE=1
"$@"
