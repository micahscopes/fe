#!/usr/bin/env python3
"""Drive and measure the zero-readback typed-CGA presentation contract."""

import argparse
import json
import math
import time

from cdp_acceptance import WebSocket, find_page


def presentation_passes(value):
    if not isinstance(value, dict):
        return False
    acceptance = value.get("acceptance")
    evidence = value.get("evidence")
    performance = value.get("performance")
    if not all(isinstance(item, dict) for item in (acceptance, evidence, performance)):
        return False
    if (acceptance.get("state") != "presentation"
            or acceptance.get("presentation") != "canvas"
            or acceptance.get("verified") is not False):
        return False
    if "wasmHash" in acceptance or "gpuHash" in acceptance:
        return False
    if evidence.get("verificationOff") is not True:
        return False
    if evidence.get("wasmWorkerCreated") is not False:
        return False
    if evidence.get("wasmOracleRenderCount") != 0 or evidence.get("gpuReadbackCount") != 0:
        return False
    if evidence.get("interactionCount", 0) < 12:
        return False
    fetched = evidence.get("fetchedAssets")
    if not isinstance(fetched, list) or any(
        asset.endswith(".wasm") or asset.endswith("reference.json") for asset in fetched
    ):
        return False
    if list(performance) != [
        "artifactFetchMs", "gpuInitMs", "firstFrameSubmitMs", "initialAcceptanceMs", "frames"
    ]:
        return False
    if performance.get("initialAcceptanceMs") is not None:
        return False
    frames = performance.get("frames")
    if not isinstance(frames, dict) or list(frames) != [
        "count", "sampleCount", "fps", "lastSubmitCpuMs", "averageSubmitCpuMs", "maxSubmitCpuMs"
    ]:
        return False
    samples = frames.get("sampleCount")
    if not isinstance(samples, int) or not 8 <= samples <= 120 or frames.get("count", 0) < samples:
        return False
    for field in ("lastSubmitCpuMs", "averageSubmitCpuMs", "maxSubmitCpuMs"):
        value = frames.get(field)
        if not isinstance(value, (int, float)) or not math.isfinite(value) or not 0 <= value < 1000:
            return False
    fps = frames.get("fps")
    return isinstance(fps, (int, float)) and math.isfinite(fps) and fps >= 0


def command(ws, command_id, method, params, deadline):
    ws.send_json({"id": command_id, "method": method, "params": params})
    while time.monotonic() < deadline:
        response = ws.recv_json()
        if response.get("id") == command_id:
            if "error" in response:
                raise RuntimeError(response["error"])
            return response.get("result", {})
    raise TimeoutError(f"CDP command {method} timed out")


def run_measurement(debug_port, page_url, timeout):
    deadline = time.monotonic() + timeout
    ws = WebSocket(find_page(debug_port, page_url, deadline), max(1, timeout))
    try:
        command_id = 0
        while time.monotonic() < deadline:
            command_id += 1
            result = command(ws, command_id, "Runtime.evaluate", {
                "expression": "JSON.stringify(window.__cgaAcceptance || null)",
                "returnByValue": True,
            }, deadline)
            raw = result.get("result", {}).get("value", "null")
            value = json.loads(raw) if isinstance(raw, str) else None
            if isinstance(value, dict) and value.get("state") == "presentation":
                break
            if isinstance(value, dict) and value.get("state") not in (None, "pending"):
                raise RuntimeError(f"presentation startup failed: {value}")
            time.sleep(0.1)
        else:
            raise TimeoutError("zero-readback presentation did not become ready")

        command_id += 1
        command(ws, command_id, "Runtime.evaluate", {
            "expression": """
              (async () => {
                const canvas = document.getElementById('view');
                const rect = canvas.getBoundingClientRect();
                for (let i = 0; i < 16; i++) {
                  canvas.dispatchEvent(new PointerEvent('pointermove', {
                    bubbles: true, isPrimary: true, pointerId: 1,
                    clientX: rect.left + 20 + i * 3,
                    clientY: rect.top + 30 + i * 2,
                  }));
                  await new Promise(requestAnimationFrame);
                  await new Promise(requestAnimationFrame);
                }
                canvas.dispatchEvent(new WheelEvent('wheel', {
                  bubbles: true, cancelable: true, deltaY: -120,
                  clientX: rect.left + rect.width / 2,
                  clientY: rect.top + rect.height / 2,
                }));
                for (let i = 0; i < 8; i++) await new Promise(requestAnimationFrame);
                return true;
              })()
            """,
            "awaitPromise": True,
            "returnByValue": True,
        }, deadline)

        command_id += 1
        result = command(ws, command_id, "Runtime.evaluate", {
            "expression": "JSON.stringify({acceptance: window.__cgaAcceptance, evidence: window.__cgaPresentationEvidence, performance: window.__cgaPerformance})",
            "returnByValue": True,
        }, deadline)
        raw = result.get("result", {}).get("value", "null")
        value = json.loads(raw) if isinstance(raw, str) else None
        print(json.dumps(value, sort_keys=True))
        return presentation_passes(value)
    finally:
        ws.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--debug-port", type=int, required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument("--timeout", type=float, default=90)
    args = parser.parse_args()
    try:
        passed = run_measurement(args.debug_port, args.url, args.timeout)
    except Exception as error:
        raise SystemExit(f"CDP presentation measurement failed: {error}")
    if not passed:
        raise SystemExit("zero-readback presentation contract did not pass")


if __name__ == "__main__":
    main()
