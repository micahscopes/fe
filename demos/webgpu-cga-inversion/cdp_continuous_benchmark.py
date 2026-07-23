#!/usr/bin/env python3
"""Assert the real-browser continuous CGA benchmark and its readback contract."""

import argparse
import json
import math
import time

from cdp_acceptance import WebSocket, find_page
from cdp_presentation import command


def _finite_nonnegative(value):
    return isinstance(value, (int, float)) and math.isfinite(value) and value >= 0


def _finite_positive(value):
    return isinstance(value, (int, float)) and math.isfinite(value) and value > 0


def benchmark_passes(value, timing="submit"):
    if not isinstance(value, dict):
        return False
    acceptance = value.get("acceptance")
    benchmark = value.get("benchmark")
    evidence = value.get("evidence")
    if not all(isinstance(item, dict) for item in (acceptance, benchmark, evidence)):
        return False
    if (acceptance.get("state") != "presentation"
            or acceptance.get("presentation") != "canvas"
            or acceptance.get("verified") is not False):
        return False
    if "wasmHash" in acceptance or "gpuHash" in acceptance:
        return False
    if acceptance.get("benchmark") != benchmark:
        return False
    if (benchmark.get("path") != "direct"
            or benchmark.get("warmupFrames") != 30
            or benchmark.get("sampleFrames") != 120):
        return False
    resolution = benchmark.get("resolution")
    if (not isinstance(resolution, dict)
            or resolution.get("width") != resolution.get("height")
            or resolution.get("width") not in (128, 256, 512, 768)
            or resolution.get("pixels") != resolution.get("width") ** 2):
        return False
    if not _finite_nonnegative(benchmark.get("averageSubmitCpuMs")):
        return False
    if not _finite_nonnegative(benchmark.get("maxSubmitCpuMs")):
        return False
    timestamp = benchmark.get("timestampQuery")
    if not isinstance(timestamp, dict):
        return False
    if (evidence.get("verificationOff") is not True
            or evidence.get("wasmWorkerCreated") is not False
            or evidence.get("wasmOracleRenderCount") != 0):
        return False
    fetched = evidence.get("fetchedAssets")
    if (not isinstance(fetched, list)
            or any(asset.endswith(".wasm") or asset.endswith("reference.json")
                   for asset in fetched)):
        return False

    if timing == "submit":
        return (
            benchmark.get("mode") == "continuous_no_readback"
            and benchmark.get("gpuCompletionMeasured") is False
            and _finite_positive(benchmark.get("submittedFrameCadenceHz"))
            and benchmark.get("completedFrameCadenceHz") is None
            and timestamp.get("available") is False
            and evidence.get("gpuReadbackCount") == 0
            and evidence.get("gpuTimestampReadbackCount") == 0
        )
    if timing == "gpu":
        return (
            benchmark.get("mode") == "continuous_gpu_timestamp"
            and benchmark.get("gpuCompletionMeasured") is True
            and benchmark.get("submittedFrameCadenceHz") is None
            and _finite_positive(benchmark.get("completedFrameCadenceHz"))
            and timestamp.get("available") is True
            and timestamp.get("samples") == 120
            and _finite_nonnegative(timestamp.get("averageGpuElapsedMs"))
            and evidence.get("gpuReadbackCount") == 150
            and evidence.get("gpuTimestampReadbackCount") == 150
        )
    return False


def run_measurement(debug_port, page_url, timeout, timing):
    deadline = time.monotonic() + timeout
    ws = WebSocket(find_page(debug_port, page_url, deadline), max(1, timeout))
    try:
        command_id = 0
        while time.monotonic() < deadline:
            command_id += 1
            result = command(ws, command_id, "Runtime.evaluate", {
                "expression": """JSON.stringify({
                  acceptance: window.__cgaAcceptance || null,
                  benchmark: window.__cgaBenchmark || null,
                  evidence: window.__cgaPresentationEvidence || null
                })""",
                "returnByValue": True,
            }, deadline)
            raw = result.get("result", {}).get("value", "null")
            value = json.loads(raw) if isinstance(raw, str) else None
            if benchmark_passes(value, timing):
                print(json.dumps(value, sort_keys=True))
                return True
            acceptance = value.get("acceptance") if isinstance(value, dict) else None
            benchmark = value.get("benchmark") if isinstance(value, dict) else None
            if (timing == "gpu" and isinstance(benchmark, dict)
                    and benchmark.get("mode") == "gpu_timestamp_unsupported"):
                raise RuntimeError(
                    f"GPU timestamp benchmark unavailable: "
                    f"{benchmark.get('timestampQuery', {}).get('reason', 'unknown reason')}"
                )
            if (isinstance(acceptance, dict)
                    and acceptance.get("state") not in (None, "pending", "presentation")):
                raise RuntimeError(f"continuous benchmark startup failed: {acceptance}")
            time.sleep(0.1)
        raise TimeoutError("continuous benchmark did not satisfy its contract")
    finally:
        ws.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--debug-port", type=int, required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument("--timing", choices=("submit", "gpu"), default="submit")
    parser.add_argument("--timeout", type=float, default=90)
    args = parser.parse_args()
    try:
        passed = run_measurement(args.debug_port, args.url, args.timeout, args.timing)
    except Exception as error:
        raise SystemExit(f"CDP continuous benchmark failed: {error}")
    if not passed:
        raise SystemExit("continuous benchmark contract did not pass")


if __name__ == "__main__":
    main()
