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

(cd "$demos" && HOST=127.0.0.1 PORT="$port" python3 serve.py) >"$tmp/server.log" 2>&1 &
server_pid=$!
url="http://127.0.0.1:$port/webgpu-cga-inversion/"
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
  --virtual-time-budget="${CGA_VIRTUAL_TIME_MS:-60000}" \
  "${webgpu_flags[@]}" "${extra_flags[@]}" \
  --dump-dom "$url" >"$tmp/dom.html" 2>"$tmp/chrome.log" &
chrome_pid=$!

deadline=$((SECONDS + ${CGA_CHROME_TIMEOUT_SECONDS:-90}))
while kill -0 "$chrome_pid" 2>/dev/null; do
  if (( SECONDS >= deadline )); then
    kill "$chrome_pid" 2>/dev/null || true
    wait "$chrome_pid" 2>/dev/null || true
    chrome_pid=""
    echo "FAIL: Chrome exceeded the smoke timeout." >&2
    tail -80 "$tmp/chrome.log" >&2 || true
    exit 1
  fi
  sleep .25
done
set +e
wait "$chrome_pid"
chrome_status=$?
set -e
chrome_pid=""
if [ "$chrome_status" -ne 0 ]; then
  echo "FAIL: Chrome exited with status $chrome_status." >&2
  tail -80 "$tmp/chrome.log" >&2 || true
  tail -40 "$tmp/server.log" >&2 || true
  exit 1
fi

if ! python3 - "$tmp/dom.html" <<'PY'
import html, json, re, sys
dom = open(sys.argv[1], encoding="utf-8").read()
status_match = re.search(r'<html[^>]*\bdata-status="([^"]+)"', dom, re.I)
accept_match = re.search(r'<pre[^>]*\bid="acceptance-json"[^>]*>(.*?)</pre>', dom, re.I | re.S)
detail_match = re.search(r'<span[^>]*\bid="detail"[^>]*>(.*?)</span>', dom, re.I | re.S)
status = html.unescape(status_match.group(1)) if status_match else "missing"
raw = html.unescape(accept_match.group(1).strip()) if accept_match else ""
detail = re.sub(r"<[^>]+>", "", html.unescape(detail_match.group(1))).strip() if detail_match else ""
try: acceptance = json.loads(raw)
except Exception: acceptance = {"state": "unparseable", "raw": raw}
print(json.dumps({"data_status": status, "acceptance": acceptance, "detail": detail}, sort_keys=True))
if status != "green" or acceptance.get("state") != "green": raise SystemExit(1)
PY
then
  echo "FAIL: browser acceptance was not GREEN." >&2
  tail -80 "$tmp/chrome.log" >&2 || true
  tail -40 "$tmp/server.log" >&2 || true
  exit 1
fi

echo "PASS: real Chrome/WebGPU D1 acceptance is GREEN."
