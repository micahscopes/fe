#!/usr/bin/env bash
# Compatibility wrapper; the repository-level command is canonical.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "${CGA_BUNDLE:-schedule32}" in
  default|d1) exec "$here/../serve.sh" cga-d1 ;;
  schedule32) exec "$here/../serve.sh" cga ;;
  *)
    echo "CGA_BUNDLE must be 'schedule32', 'd1', or legacy alias 'default'" >&2
    exit 2
    ;;
esac
