#!/usr/bin/env bash
# Serialize operations that temporarily rewrite the Fe checkout's Cargo.lock.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
if [ "$#" -eq 0 ]; then
  echo "usage: $0 COMMAND [ARG ...]" >&2
  exit 2
fi

state_root="${FE_DEMO_STATE_DIR:-$repo/target/fe-demo-state}"
mkdir -p "$state_root"
demo_tmp_root="${FE_DEMO_TMPDIR:-$repo/output/demo-tmp}"
mkdir -p "$demo_tmp_root"
exec 8>"$state_root/generation.lock"
flock 8
export FE_DEMO_GENERATION_LOCK_ACTIVE=1
export FE_DEMO_TMPDIR="$demo_tmp_root"
# Cargo build scripts, native compilers, and linkers consult TMPDIR directly.
# Keep their temporary objects under the workspace as well as our own overlay
# and lock-backup files.
export TMPDIR="$demo_tmp_root"
"$@"
