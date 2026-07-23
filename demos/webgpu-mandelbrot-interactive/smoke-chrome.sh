#!/usr/bin/env bash
# Hosted module-Worker + WebGPU/Wasm acceptance under Chrome SwiftShader.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
demos="$(cd "$here/.." && pwd)"
repo="$(cd "$demos/.." && pwd)"
demo_tmp_root="${FE_DEMO_TMPDIR:-$repo/output/demo-tmp}"
mkdir -p "$demo_tmp_root"
tmp="$(mktemp -d "$demo_tmp_root/fe-mandel-chrome.XXXXXX")"
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
  echo "UNAVAILABLE: set CHROME_BIN to Chrome/Chromium with WebGPU support." >&2
  exit 69
fi

for asset in ctl.json ctl.wasm frag.wasm frag.wgsl layout.json reference.json; do
  if [ ! -f "$here/gen/$asset" ]; then
    echo "Missing hosted Mandelbrot asset gen/$asset; regenerate the interactive demo bundle." >&2
    exit 2
  fi
done

read -r port debug_port < <(python3 - <<'PY'
import socket
ports = []
for _ in range(2):
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        ports.append(sock.getsockname()[1])
print(*ports)
PY
)
NO_COLOR=false trunk serve --config "$demos/Trunk.toml" --port "$port" --no-autoreload \
  >"$tmp/server.log" 2>&1 &
server_pid=$!
url="http://127.0.0.1:$port/webgpu-mandelbrot-interactive/?acceptance=offscreen"

python3 - "$url" "$server_pid" <<'PY'
import os, sys, time, urllib.request
url, pid = sys.argv[1], int(sys.argv[2])
for _ in range(100):
    try:
        with urllib.request.urlopen(url, timeout=.5) as response:
            if response.status == 200: raise SystemExit(0)
    except Exception:
        try: os.kill(pid, 0)
        except OSError: raise SystemExit("demo server exited before becoming ready")
        time.sleep(.1)
raise SystemExit(f"demo server did not become ready: {url}")
PY

webgpu_flags=()
read -r -a webgpu_flags <<<"${MANDEL_CHROME_WEBGPU_FLAGS:---enable-unsafe-webgpu --use-angle=swiftshader}" || true
"$chrome" --headless=new --no-first-run --no-default-browser-check \
  --user-data-dir="$tmp/profile" --remote-debugging-address=127.0.0.1 \
  --remote-debugging-port="$debug_port" "${webgpu_flags[@]}" "$url" \
  >"$tmp/chrome.out" 2>"$tmp/chrome.log" &
chrome_pid=$!

if ! python3 "$here/cdp_acceptance.py" --debug-port "$debug_port" --url "$url" \
  --timeout "${MANDEL_CHROME_TIMEOUT_SECONDS:-120}"; then
  tail -80 "$tmp/chrome.log" >&2 || true
  tail -40 "$tmp/server.log" >&2 || true
  exit 1
fi
echo "PASS: hosted Mandelbrot module Worker completed 4,007 controls and SwiftShader GPU/Wasm verification."
