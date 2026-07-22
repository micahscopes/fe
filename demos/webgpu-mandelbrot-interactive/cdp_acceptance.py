#!/usr/bin/env python3
"""Poll hosted Mandelbrot's structured real-browser result over CDP."""

import argparse
import importlib.util
import json
import time
from pathlib import Path

_SHARED_PATH = Path(__file__).resolve().parents[1] / "webgpu-cga-inversion" / "cdp_acceptance.py"
_SHARED_SPEC = importlib.util.spec_from_file_location("cga_cdp_transport", _SHARED_PATH)
if _SHARED_SPEC is None or _SHARED_SPEC.loader is None:
    raise RuntimeError(f"cannot load shared CDP transport from {_SHARED_PATH}")
_SHARED = importlib.util.module_from_spec(_SHARED_SPEC)
_SHARED_SPEC.loader.exec_module(_SHARED)
WebSocket = _SHARED.WebSocket
find_page = _SHARED.find_page


def acceptance_passes(value):
    return (
        isinstance(value, dict)
        and value.get("state") == "green"
        and value.get("worker") is True
        and value.get("presentation") == "offscreen"
        and value.get("controlsSteps") == 4007
        and value.get("verified") is True
        and isinstance(value.get("adapter"), str)
        and bool(value["adapter"])
        and value.get("gpuHash") == value.get("wasmHash") == value.get("referenceHash")
    )


def poll_acceptance(debug_port, page_url, timeout):
    deadline = time.monotonic() + timeout
    ws = WebSocket(find_page(debug_port, page_url, deadline), max(1, timeout))
    command_id = 0
    try:
        while time.monotonic() < deadline:
            command_id += 1
            ws.send_json({
                "id": command_id,
                "method": "Runtime.evaluate",
                "params": {
                    "expression": "JSON.stringify(window.__mandelAcceptance || null)",
                    "returnByValue": True,
                },
            })
            while time.monotonic() < deadline:
                response = ws.recv_json()
                if response.get("id") != command_id:
                    continue
                raw = response.get("result", {}).get("result", {}).get("value", "null")
                value = json.loads(raw) if isinstance(raw, str) else None
                if isinstance(value, dict) and value.get("state") != "pending":
                    print(json.dumps(value, sort_keys=True))
                    return acceptance_passes(value)
                break
            time.sleep(0.1)
    finally:
        ws.close()
    raise TimeoutError("hosted Mandelbrot acceptance remained pending")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--debug-port", type=int, required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument("--timeout", type=float, default=120)
    args = parser.parse_args()
    try:
        passed = poll_acceptance(args.debug_port, args.url, args.timeout)
    except Exception as error:
        raise SystemExit(f"CDP acceptance failed: {error}")
    if not passed:
        raise SystemExit("Mandelbrot did not satisfy worker + oracle + GPU/Wasm acceptance")


if __name__ == "__main__":
    main()
