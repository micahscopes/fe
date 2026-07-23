#!/usr/bin/env bash
# Compatibility wrapper; the repository-level command is canonical.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$here/../serve.sh" keystone
