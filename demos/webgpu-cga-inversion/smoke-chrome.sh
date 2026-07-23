#!/usr/bin/env bash
# Real-browser typed-CGA smoke. Exit 0 means the selected contract passed;
# exit 69 means no browser. `verify=off` is presentation, never green acceptance.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
demos="$(cd "$here/.." && pwd)"
repo="$(cd "$demos/.." && pwd)"
demo_tmp_root="${FE_DEMO_TMPDIR:-$repo/output/demo-tmp}"
mkdir -p "$demo_tmp_root"
tmp="$(mktemp -d "$demo_tmp_root/fe-cga-chrome.XXXXXX")"
server_pid=""
chrome_pid=""

cleanup() {
  if [ -n "$chrome_pid" ]; then kill "$chrome_pid" 2>/dev/null || true; fi
  if [ -n "$server_pid" ]; then kill "$server_pid" 2>/dev/null || true; fi
  wait "$chrome_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
  rm -rf -- "$tmp"
}
trap cleanup EXIT INT TERM

chrome="${CHROME_BIN:-}"
if [ -z "$chrome" ]; then
  for candidate in google-chrome-stable google-chrome chromium chromium-browser; do
    if command -v "$candidate" >/dev/null 2>&1; then chrome="$(command -v "$candidate")"; break; fi
  done
fi
if [ -z "$chrome" ] || [ ! -x "$chrome" ]; then
  echo "UNAVAILABLE: set CHROME_BIN to an executable Chrome/Chromium with WebGPU support." >&2
  exit 69
fi

requested_bundle="${CGA_SMOKE_BUNDLE:-}"
case "$requested_bundle" in
  ""|schedule32)
    CGA_BUNDLE=schedule32 \
      CGA_BUNDLE_DIR="$here/gen-schedule32" \
      "$here/ensure-assets.sh"
    ;;
  d1|legacy)
    CGA_BUNDLE=default \
      CGA_BUNDLE_DIR="$here/gen" \
      "$here/ensure-assets.sh"
    ;;
  *) echo "CGA_SMOKE_BUNDLE must be 'd1', 'legacy', or 'schedule32'" >&2; exit 2 ;;
esac

port="${CGA_SMOKE_PORT:-}"
if [ -z "$port" ]; then
  port="$(python3 - <<'PY'
import socket
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"
fi
case "$port" in *[!0-9]*|'') echo "CGA_SMOKE_PORT must be numeric" >&2; exit 2;; esac

debug_port="${CGA_CDP_PORT:-}"
if [ -z "$debug_port" ]; then
  debug_port="$(python3 - <<'PY'
import socket
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"
fi
case "$debug_port" in *[!0-9]*|'') echo "CGA_CDP_PORT must be numeric" >&2; exit 2;; esac

NO_COLOR=false trunk serve --config "$demos/Trunk.toml" --port "$port" --no-autoreload \
  >"$tmp/server.log" 2>&1 &
server_pid=$!
default_presentation="offscreen"
if [ "${CGA_SMOKE_VERIFY:-default}" = "off" ]; then default_presentation="canvas"; fi
presentation="${CGA_SMOKE_PRESENTATION:-$default_presentation}"
case "$presentation" in
  offscreen) query="acceptance=offscreen" ;;
  canvas) query="" ;;
  *) echo "CGA_SMOKE_PRESENTATION must be 'offscreen' or 'canvas'" >&2; exit 2 ;;
esac
verify_mode="${CGA_SMOKE_VERIFY:-default}"
case "$verify_mode" in
  default) expected_state="green" ;;
  off)
    expected_state="presentation"
    query="${query:+$query&}verify=off"
    ;;
  continuous)
    expected_state="green"
    query="${query:+$query&}verify=continuous"
    ;;
  *) echo "CGA_SMOKE_VERIFY must be 'default', 'off', or 'continuous'" >&2; exit 2 ;;
esac
append_query() {
  query="${query:+$query&}$1=$2"
}
bundle="$requested_bundle"
case "$bundle" in
  "") ;;
  d1|legacy|schedule32) append_query bundle "$bundle" ;;
esac
benchmark="${CGA_SMOKE_BENCHMARK:-}"
case "$benchmark" in
  "") ;;
  continuous) append_query benchmark "$benchmark" ;;
  *) echo "CGA_SMOKE_BENCHMARK must be 'continuous'" >&2; exit 2 ;;
esac
resolution="${CGA_SMOKE_RESOLUTION:-}"
case "$resolution" in
  "") ;;
  128|256|512|768) append_query resolution "$resolution" ;;
  *) echo "CGA_SMOKE_RESOLUTION must be one of 128, 256, 512, or 768" >&2; exit 2 ;;
esac
timing="${CGA_SMOKE_TIMING:-}"
case "$timing" in
  "") ;;
  gpu) append_query timing "$timing" ;;
  *) echo "CGA_SMOKE_TIMING must be 'gpu'" >&2; exit 2 ;;
esac
lifecycle="${CGA_SMOKE_LIFECYCLE:-}"
case "$lifecycle" in
  "") ;;
  1) append_query smoke lifecycle ;;
  *) echo "CGA_SMOKE_LIFECYCLE must be '1' or empty" >&2; exit 2 ;;
esac
if [ "$benchmark" = "continuous" ] \
    && { [ "$verify_mode" != "off" ] || [ "$presentation" != "canvas" ]; }; then
  echo "CGA_SMOKE_BENCHMARK=continuous requires CGA_SMOKE_VERIFY=off and canvas presentation" >&2
  exit 2
fi
if [ "$timing" = "gpu" ] && [ "$benchmark" != "continuous" ]; then
  echo "CGA_SMOKE_TIMING=gpu requires CGA_SMOKE_BENCHMARK=continuous" >&2
  exit 2
fi
if [ "$lifecycle" = 1 ] \
    && { [ "$verify_mode" != "default" ] \
      || [ -n "$benchmark" ] \
      || [ -n "$timing" ] \
      || { [ -n "$requested_bundle" ] && [ "$requested_bundle" != "schedule32" ]; }; }; then
  echo "CGA_SMOKE_LIFECYCLE=1 requires verified Schedule32 mode without a benchmark" >&2
  exit 2
fi
url="http://127.0.0.1:$port/webgpu-cga-inversion/${query:+?$query}"
python3 - "$url" "$server_pid" <<'PY'
import os, sys, time, urllib.request
url, pid = sys.argv[1], int(sys.argv[2])
for _ in range(100):
    try:
        with urllib.request.urlopen(url, timeout=.5) as response:
            if response.status == 200:
                raise SystemExit(0)
    except Exception:
        try: os.kill(pid, 0)
        except OSError: raise SystemExit("demo server exited before becoming ready")
        time.sleep(.1)
raise SystemExit(f"demo server did not become ready: {url}")
PY

# Override with CHROME_WEBGPU_FLAGS when a platform needs native Metal/D3D flags.
default_flags="--enable-unsafe-webgpu --enable-features=Vulkan --use-angle=vulkan"
webgpu_flags=()
extra_flags=()
read -r -a webgpu_flags <<<"${CHROME_WEBGPU_FLAGS:-$default_flags}" || true
if [ -n "${CHROME_EXTRA_FLAGS:-}" ]; then
  read -r -a extra_flags <<<"$CHROME_EXTRA_FLAGS" || true
fi
"$chrome" \
  --headless=new \
  --no-first-run \
  --no-default-browser-check \
  --user-data-dir="$tmp/profile" \
  --remote-debugging-address=127.0.0.1 \
  --remote-debugging-port="$debug_port" \
  "${webgpu_flags[@]}" "${extra_flags[@]}" \
  "$url" >"$tmp/chrome.out" 2>"$tmp/chrome.log" &
chrome_pid=$!

set +e
if [ "$lifecycle" = 1 ]; then
  python3 "$here/cdp_lifecycle.py" \
    --debug-port "$debug_port" --url "$url" \
    --timeout "${CGA_CHROME_TIMEOUT_SECONDS:-90}"
elif [ "$benchmark" = "continuous" ]; then
  python3 "$here/cdp_continuous_benchmark.py" \
    --debug-port "$debug_port" --url "$url" \
    --timing "${timing:-submit}" \
    --timeout "${CGA_CHROME_TIMEOUT_SECONDS:-90}"
elif [ "$verify_mode" = "off" ]; then
  python3 "$here/cdp_presentation.py" \
    --debug-port "$debug_port" --url "$url" \
    --timeout "${CGA_CHROME_TIMEOUT_SECONDS:-90}"
else
  python3 "$here/cdp_acceptance.py" \
    --debug-port "$debug_port" \
    --url "$url" \
    --presentation "$presentation" \
    --expected-state "$expected_state" \
    --timeout "${CGA_CHROME_TIMEOUT_SECONDS:-90}"
fi
acceptance_status=$?
set -e
if [ "$acceptance_status" -ne 0 ]; then
  echo "FAIL: browser did not satisfy typed-CGA mode '$verify_mode' (expected state '$expected_state')." >&2
  tail -80 "$tmp/chrome.log" >&2 || true
  tail -40 "$tmp/server.log" >&2 || true
  exit 1
fi

kill "$chrome_pid" 2>/dev/null || true
wait "$chrome_pid" 2>/dev/null || true
chrome_pid=""

if [ "$expected_state" = "green" ]; then
  if [ "$lifecycle" = 1 ]; then
    echo "PASS: real Chrome/WebGPU typed-CGA generated actor lifecycle is GREEN."
  else
    echo "PASS: real Chrome/WebGPU typed-CGA $presentation acceptance is GREEN ($verify_mode verification)."
  fi
else
  echo "PASS: real Chrome/WebGPU typed-CGA $presentation showcase is PRESENTATION/UNVERIFIED (verification off)."
fi
