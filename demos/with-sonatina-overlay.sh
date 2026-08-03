#!/usr/bin/env bash
# Run a command against the reviewed Sonatina overlay without mutating either
# the Fe workspace or a caller-supplied Sonatina checkout.
set -euo pipefail

archive_base="150d327edfa88374802a6cc8089fd77da5fa818b"
archive_head="ac266c210cad7872fc98380a73b4ca363877bc1f"
pinned_base="43e9f3b0d60fff4f8f7006174b9f1d406a0c70f0"
reviewed_heads=(
  "548b7e54b64dcef19fa4383a786a620db2300d9f"
  "7ced9661ba2f340684b7d927a1318b147e54c851"
  "09e2895e37d120a90ea889017ad730e1dbf0f7a3"
  "a170ef047a1a16ae170f9bbbe3b9dfa9879edf15"
)
reviewed_committer_dates=(
  "2026-07-30T14:44:18+02:00"
  "2026-07-30T15:45:18+02:00"
  "2026-07-30T16:10:31+02:00"
  "2026-07-31T20:30:54+02:00"
)
remote="https://github.com/micahscopes/sonatina.git"
remote_ref="refs/heads/mb2-render-mode"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
patches="$repo/vendor/sonatina/mb2-browser-runtime"

die() {
  echo "with-sonatina-overlay: $*" >&2
  exit "${2:-1}"
}

if [ "$#" -eq 0 ]; then
  echo "usage: $0 COMMAND [ARG ...]" >&2
  exit 2
fi

if ! (cd "$patches" && sha256sum --check --quiet SHA256SUMS); then
  die "tracked patch checksum mismatch"
fi

source_dir="${SONATINA_DIR:-}"
if [ -n "$source_dir" ]; then
  git -C "$source_dir" rev-parse --git-dir >/dev/null 2>&1 ||
    die "SONATINA_DIR must identify a Sonatina git checkout" 2
  if [ -n "$(git -C "$source_dir" status --porcelain 2>/dev/null)" ]; then
    die "SONATINA_DIR must be clean; the checkout will never be modified" 2
  fi
else
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
    die "cached Sonatina origin is not the pinned remote $remote"
  fi
  if ! git -C "$source_dir" cat-file -e "$archive_base^{commit}" 2>/dev/null ||
    ! git -C "$source_dir" cat-file -e "$pinned_base^{commit}" 2>/dev/null; then
    if [ "${FE_BROWSER_OFFLINE:-0}" = 1 ]; then
      die "offline cache is missing Sonatina bases $archive_base and/or $pinned_base" 2
    fi
    echo "fetching the pinned Sonatina history..." >&2
    git -C "$source_dir" fetch --quiet origin "$remote_ref"
  fi
  flock --unlock 9
fi

for object in "$archive_base" "$archive_head" "$pinned_base"; do
  git -C "$source_dir" cat-file -e "$object^{commit}" 2>/dev/null ||
    die "Sonatina source does not contain required commit $object" 2
done
git -C "$source_dir" merge-base --is-ancestor "$archive_head" "$pinned_base" ||
  die "pinned Sonatina base is not descended from reviewed archive head"

tmp_root="${FE_DEMO_TMPDIR:-$repo/output/demo-tmp}"
mkdir -p "$tmp_root"
overlay_root="$(mktemp -d "$tmp_root/fe-sonatina-overlay.XXXXXX")"
cleanup() {
  case "$overlay_root" in
    "$tmp_root"/fe-sonatina-overlay.*) rm -rf -- "$overlay_root" ;;
    *) echo "refusing to remove unexpected overlay path: $overlay_root" >&2 ;;
  esac
}
trap cleanup EXIT

overlay="$overlay_root/sonatina"
git clone --quiet --shared --no-checkout "$source_dir" "$overlay"

# The historical archive and current candidates have a deliberate bridge:
# 0001–0028 reconstruct ac266c21 from the old browser branch, while Fe's exact
# pin 43e9f3b0 contains that commit plus two published scalar-op commits.
git -C "$overlay" checkout --quiet --detach "$archive_base"
git -C "$overlay" -c user.name=trial -c user.email=trial@local \
  am --quiet --committer-date-is-author-date "$patches"/00{01..28}-*.patch
[ "$(git -C "$overlay" rev-parse HEAD)" = "$archive_head" ] ||
  die "patches 0001–0028 did not reconstruct $archive_head"

git -C "$overlay" checkout --quiet --detach "$pinned_base"
patch_number=29
patch_index=0
for expected in "${reviewed_heads[@]}"; do
  patch="$(printf '%s/%04d-' "$patches" "$patch_number")"
  patch="$(compgen -G "${patch}*.patch" || true)"
  [ -n "$patch" ] || die "missing reviewed patch $(printf '%04d' "$patch_number")"
  GIT_COMMITTER_DATE="${reviewed_committer_dates[$patch_index]}" \
    git -C "$overlay" -c user.name=Codex -c user.email=codex@local \
    am --quiet "$patch"
  actual="$(git -C "$overlay" rev-parse HEAD)"
  case "$actual" in
    "$expected"*) ;;
    *) die "patch $(printf '%04d' "$patch_number") reconstructed $actual, expected $expected" ;;
  esac
  patch_number=$((patch_number + 1))
  patch_index=$((patch_index + 1))
done

[ -z "$(git -C "$overlay" status --porcelain)" ] ||
  die "reconstructed overlay is unexpectedly dirty"

export SONATINA_DIR="$overlay"
export FE_SONATINA_OVERLAY_ACTIVE=1
export FE_SONATINA_OVERLAY_COMMIT="${reviewed_heads[3]}"

patch_args=(
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-ir.path=\"$overlay/crates/ir\""
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-triple.path=\"$overlay/crates/triple\""
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-codegen.path=\"$overlay/crates/codegen\""
  --config "patch.\"https://github.com/micahscopes/sonatina\".sonatina-verifier.path=\"$overlay/crates/verifier\""
)

if [ "$(basename "$1")" = cargo ]; then
  cargo_command="$1"
  shift
  # Cargo needs to resolve the path-patched packages into a lockfile. Do that in
  # a reflink-friendly source snapshot so neither Cargo.lock nor generated
  # source in the caller's checkout can change.
  proof_workspace="$overlay_root/fe"
  rsync -a --delete \
    --exclude '/.git/' \
    --exclude '/target/' \
    --exclude '/target-*/' \
    --exclude '/output/' \
    "$repo/" "$proof_workspace/"
  case "$PWD/" in
    "$repo/"*) relative_cwd="${PWD#"$repo"}" ;;
    *) relative_cwd="" ;;
  esac
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo/target-sonatina-overlay}"
  (
    cd "$proof_workspace$relative_cwd"
    "$cargo_command" "${patch_args[@]}" "$@"
  )
else
  # Non-Cargo wrappers receive SONATINA_DIR and can use the same four --config
  # entries. Existing demo generators already consume SONATINA_DIR this way.
  "$@"
fi
