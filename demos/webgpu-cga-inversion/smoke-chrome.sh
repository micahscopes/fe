#!/usr/bin/env bash
# Real-browser D1 smoke gate. Exit 0 means GREEN; exit 69 means no browser.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
demos="$(cd "$here/.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/fe-cga-chrome.XXXXXX")"
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

python3 "$here/verify-assets.py"

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

(cd "$demos" && HOST=127.0.0.1 PORT="$port" python3 serve.py) >"$tmp/server.log" 2>&1 &
server_pid=$!
presentation="${CGA_SMOKE_PRESENTATION:-offscreen}"
case "$presentation" in
  offscreen) query="?acceptance=offscreen" ;;
  canvas) query="" ;;
  *) echo "CGA_SMOKE_PRESENTATION must be 'offscreen' or 'canvas'" >&2; exit 2 ;;
esac
url="http://127.0.0.1:$port/webgpu-cga-inversion/$query"
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
python3 "$here/cdp_acceptance.py" \
  --debug-port "$debug_port" \
  --url "$url" \
  --presentation "$presentation" \
  --timeout "${CGA_CHROME_TIMEOUT_SECONDS:-90}"
acceptance_status=$?
set -e
if [ "$acceptance_status" -ne 0 ]; then
  echo "FAIL: browser acceptance did not become GREEN." >&2
  tail -80 "$tmp/chrome.log" >&2 || true
  tail -40 "$tmp/server.log" >&2 || true
  exit 1
fi

kill "$chrome_pid" 2>/dev/null || true
wait "$chrome_pid" 2>/dev/null || true
chrome_pid=""

echo "PASS: real Chrome/WebGPU D1 $presentation acceptance is GREEN."
