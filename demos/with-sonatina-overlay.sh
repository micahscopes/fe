#!/usr/bin/env bash
# Reconstruct the reviewed browser-backend Sonatina commit from tracked patches,
# without modifying the caller's checkout.
set -euo pipefail

base="150d327edfa88374802a6cc8089fd77da5fa818b"
target="ac266c210cad7872fc98380a73b4ca363877bc1f"
remote="https://github.com/micahscopes/sonatina.git"
remote_ref="refs/heads/mb2-render-mode"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
patches="$repo/vendor/sonatina/mb2-browser-runtime"

if [ "$#" -eq 0 ]; then
  echo "usage: $0 COMMAND [ARG ...]" >&2
  exit 2
fi

source_dir="${SONATINA_DIR:-}"
if [ -z "$source_dir" ]; then
  cache_root="${FE_BROWSER_CACHE_DIR:-$repo/target/fe-browser-cache}"
  source_dir="$cache_root/sonatina.git"
  mkdir -p "$cache_root"
  exec 9>"$cache_root/sonatina.lock"
  flock 9
  if [ ! -d "$source_dir" ]; then
    git init --quiet --bare "$source_dir"
  fi
  if ! git -C "$source_dir" remote get-url origin >/dev/null 2>&1; then
    git -C "$source_dir" remote add origin "$remote"
  elif [ "$(git -C "$source_dir" remote get-url origin)" != "$remote" ]; then
    echo "cached Sonatina origin is not the pinned remote $remote" >&2
    exit 1
  fi
  if ! git -C "$source_dir" cat-file -e "$base^{commit}" 2>/dev/null; then
    if [ "${FE_BROWSER_OFFLINE:-0}" = 1 ]; then
      echo "offline browser build cache is missing Sonatina base $base" >&2
      exit 2
    fi
    echo "fetching pinned Sonatina browser base $base..." >&2
    git -C "$source_dir" fetch --quiet --depth=1 origin \
      "$remote_ref:refs/heads/fe-browser-base"
  fi
  git -C "$source_dir" update-ref refs/heads/fe-browser-base "$base"
  cached_base="$(git -C "$source_dir" rev-parse refs/heads/fe-browser-base 2>/dev/null || true)"
  if [ "$cached_base" != "$base" ]; then
    echo "pinned Sonatina ref $remote_ref resolved to ${cached_base:-nothing}, expected $base" >&2
    exit 1
  fi
  flock --unlock 9
elif ! git -C "$source_dir" rev-parse --git-dir >/dev/null 2>&1; then
  echo "SONATINA_DIR must identify a Sonatina git checkout containing $base" >&2
  exit 2
fi
if ! git -C "$source_dir" cat-file -e "$base^{commit}" 2>/dev/null; then
  echo "SONATINA_DIR does not contain browser-backend base $base" >&2
  exit 2
fi
if ! (cd "$patches" && sha256sum --check --quiet SHA256SUMS); then
  echo "tracked Sonatina browser-backend patch checksum mismatch" >&2
  exit 1
fi

actual="$(git -C "$source_dir" rev-parse HEAD 2>/dev/null || true)"
if [ "$actual" = "$target" ] && [ -z "$(git -C "$source_dir" status --porcelain)" ]; then
  export SONATINA_DIR="$source_dir"
  exec "$@"
fi

demo_tmp_root="${FE_DEMO_TMPDIR:-$repo/output/demo-tmp}"
mkdir -p "$demo_tmp_root"
overlay_root="$(mktemp -d "$demo_tmp_root/fe-sonatina-overlay.XXXXXX")"
cleanup() {
  case "$overlay_root" in
    "$demo_tmp_root"/fe-sonatina-overlay.*) rm -rf -- "$overlay_root" ;;
    *) echo "refusing to remove unexpected overlay path: $overlay_root" >&2 ;;
  esac
}
trap cleanup EXIT

overlay="$overlay_root/sonatina"
git clone --quiet --shared --no-checkout "$source_dir" "$overlay"
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
