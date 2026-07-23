#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
repo="$(cd "$root/.." && pwd)"
chrome="${CHROME_BIN:-}"
if [ -z "$chrome" ]; then for c in google-chrome-stable google-chrome chromium chromium-browser; do command -v "$c" >/dev/null 2>&1 && chrome="$(command -v "$c")" && break; done; fi
if [ -z "$chrome" ]; then echo "Chrome/Chromium unavailable" >&2; exit 69; fi
demo_tmp_root="${FE_DEMO_TMPDIR:-$repo/output/demo-tmp}"
mkdir -p "$demo_tmp_root"
tmp="$(mktemp -d "$demo_tmp_root/qcga-chrome.XXXXXX")"; mode="${QCGA_MODE:-verify}"
read -r port debug < <(python3 - <<'PY'
import socket
ports=[]
for _ in range(2):
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0)); ports.append(sock.getsockname()[1])
print(*ports)
PY
)
port="${QCGA_PORT:-$port}"; debug="${QCGA_DEBUG_PORT:-$debug}"
cleanup(){ kill "${chrome_pid:-}" "${server_pid:-}" 2>/dev/null || true; wait "${chrome_pid:-}" "${server_pid:-}" 2>/dev/null || true; rm -rf "$tmp" || true; }; trap cleanup EXIT
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$root" >"$tmp/http.log" 2>&1 & server_pid=$!
query="acceptance=offscreen"; [ "$mode" = off ] && query="verify=off"
url="http://127.0.0.1:$port/webgpu-qcga3d-quadric/?$query"
default_flags="--enable-unsafe-webgpu --enable-features=Vulkan --use-vulkan=swiftshader --disable-vulkan-surface"
read -r -a webgpu_flags <<<"${CHROME_WEBGPU_FLAGS:-$default_flags}" || true
"$chrome" --headless=new --no-sandbox --disable-dev-shm-usage \
  "${webgpu_flags[@]}" --remote-debugging-port="$debug" \
  --user-data-dir="$tmp/profile" "$url" >"$tmp/chrome.out" 2>"$tmp/chrome.log" & chrome_pid=$!
set +e
python3 "$here/cdp_acceptance.py" --debug-port "$debug" --url "$url" --mode "$mode" --timeout 90
acceptance_status=$?
set -e
if [ "$acceptance_status" -ne 0 ]; then
  tail -80 "$tmp/chrome.log" >&2 || true
  exit "$acceptance_status"
fi
echo "PASS: Chrome SwiftShader QCGA mode=$mode"
