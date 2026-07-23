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
exec 8>"$state_root/generation.lock"
flock 8
export FE_DEMO_GENERATION_LOCK_ACTIVE=1
"$@"
