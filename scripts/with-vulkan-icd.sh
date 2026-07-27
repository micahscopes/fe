#!/usr/bin/env bash
# Run a command with a software-Vulkan ICD wired up, so the SPIR-V execution
# tests can actually execute.
#
# Why this exists: on 2026-07-27 the full CI run reported 23 `spirv_e2e`
# failures, and STATE.md item 3 recorded them as "no Vulkan adapter on this
# host", offering the choice "install a software ICD, or record here that
# SPIR-V is UNVERIFIABLE on this machine". Neither was needed. Mesa's lavapipe
# (`libvulkan_lvp.so` plus `lvp_icd.x86_64.json`) and the Vulkan loader were
# ALREADY present in the nix store. Nothing pointed `VK_ICD_FILENAMES` at them.
# With the ICD wired, `spirv_e2e` runs 43 passed / 0 failed in 154 seconds.
#
# The absence of `/dev/dri` is what made "no GPU" look true. It is not
# evidence: lavapipe is a CPU rasterizer and never opens a DRM node.
# `.config/nextest.toml` already carried a `lavapipe` test-group serializing
# these tests "because the lavapipe device is single-threaded software
# rendering", which was the standing evidence that this host was expected to
# render in software all along.
#
# The store paths are content-hashed, so they cannot be committed as literals.
# This script discovers them instead. If discovery fails it says so and runs
# the command anyway, so a host with no software Vulkan degrades to the old
# behaviour rather than silently pretending.
#
# Usage:
#   scripts/with-vulkan-icd.sh cargo nextest run --release -p fe-codegen ...
#
# Everything rendered under this ICD is SOFTWARE rendered (llvmpipe). No
# performance claim attaches to any result obtained through it, per the
# VISION 3.1 registers.

set -euo pipefail

if [[ -z "${VK_ICD_FILENAMES:-}" ]]; then
  icd="$(ls -1 /nix/store/*mesa-*/share/vulkan/icd.d/lvp_icd.x86_64.json 2>/dev/null | head -1 || true)"
  if [[ -n "$icd" ]]; then
    export VK_ICD_FILENAMES="$icd"
  else
    echo "with-vulkan-icd: no lavapipe ICD found; SPIR-V execution tests will not run" >&2
  fi
fi

loader_dir="$(ls -1d /nix/store/*vulkan-loader-*/lib 2>/dev/null | head -1 || true)"
if [[ -n "$loader_dir" ]]; then
  export LD_LIBRARY_PATH="${loader_dir}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
fi

exec "$@"
